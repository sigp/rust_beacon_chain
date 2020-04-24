//! Provides verification for the following attestations:
//!
//! - "Unaggregated" `Attestation` received from either gossip or the HTTP API.
//! - "Aggregated" `SignedAggregateAndProof` received from gossip or the HTTP API.
//!
//! For clarity, we define:
//!
//! - Unaggregated: an `Attestation` object that has exactly one aggregation bit set.
//! - Aggregated: a `SignedAggregateAndProof` which has zero or more signatures.
//!   - Note: "zero or more" may soon change to "one or more".
//!
//! Similar to the `crate::block_verification` module, we try to avoid doing duplicate verification
//! work as an attestation passes through different stages of verification. We represent these
//! different stages of verification with wrapper types. These wrapper-types flow in a particular
//! pattern:
//!
//! ```ignore
//!      types::Attestation              types::SignedAggregateAndProof
//!              |                                    |
//!              ▼                                    ▼
//!  VerifiedUnaggregatedAttestation     VerifiedAggregatedAttestation
//!              |                                    |
//!              -------------------------------------
//!                                |
//!                                ▼
//!                  ForkChoiceVerifiedAttestation
//! ```

use crate::{
    beacon_chain::{
        ATTESTATION_CACHE_LOCK_TIMEOUT, HEAD_LOCK_TIMEOUT, MAXIMUM_GOSSIP_CLOCK_DISPARITY,
        VALIDATOR_PUBKEY_CACHE_LOCK_TIMEOUT,
    },
    metrics,
    observed_attestations::ObserveOutcome,
    observed_attesters::Error as ObservedAttestersError,
    BeaconChain, BeaconChainError, BeaconChainTypes,
};
use bls::verify_signature_sets;
use slog::debug;
use slot_clock::SlotClock;
use state_processing::{
    common::get_indexed_attestation,
    per_block_processing::errors::AttestationValidationError,
    per_slot_processing,
    signature_sets::{
        indexed_attestation_signature_set_from_pubkeys,
        signed_aggregate_selection_proof_signature_set, signed_aggregate_signature_set,
    },
};
use std::borrow::Cow;
use tree_hash::TreeHash;
use types::{
    Attestation, BeaconCommittee, CommitteeIndex, Epoch, EthSpec, Hash256, IndexedAttestation,
    RelativeEpoch, SelectionProof, SignedAggregateAndProof, Slot,
};

/// Returned when an attestation was not declared to be valid. It may not be found to be valid for
/// two reasons:
///
/// - The attestation is malformed or inappropriate for the context (indicated by all variants
///   other than `BeaconChainError`.
/// - The application encountered an internal error whilst attempting to determine validity
///   (the `BeaconChainError` variant)
#[derive(Debug, PartialEq)]
pub enum Error {
    /// The attestation is from a slot that is later than the current slot (with respect to the
    /// gossip clock disparity).
    FutureSlot {
        attestation_slot: Slot,
        latest_permissible_slot: Slot,
    },
    /// The attestation is from a slot that is prior to the earliest permissible slolt (with
    /// respect to the gossip clock disparity).
    PastSlot {
        attestation_slot: Slot,
        earliest_permissible_slot: Slot,
    },
    /// The attestations aggregation bits were empty when they shouldn't be.
    EmptyAggregationBitfield,
    /// The `selection_proof` on the aggregate attestation does not elect it as an aggregator.
    InvalidSelectionProof {
        aggregator_index: u64,
    },
    /// The `selection_proof` on the aggregate attestation selects it as a validator, however the
    /// aggregator index is not in the committee for that attestation.
    AggregatorNotInCommittee {
        aggregator_index: u64,
    },
    /// The aggregator index refers to a validator index that we have not seen.
    AggregatorPubkeyUnknown(u64),
    /// The attestation has been seen before; either in a block, on the gossip network or from a
    /// local validator.
    AttestationAlreadyKnown(Hash256),
    /// There has already been an aggregation observed for this validator, we refuse to process a
    /// second.
    AggregatorAlreadyKnown(u64),
    /// The aggregator index is higher than the maximum possible validator count.
    ValidatorIndexTooHigh(usize),
    /// The `attestation.data.beacon_block_root` block is unknown.
    UnknownHeadBlock {
        beacon_block_root: Hash256,
    },
    /// The target epoch for the attestation is not in the same epoch as the slot it was produced
    /// for.
    BadTargetEpoch,
    /// The target root of the attestation points to a block that we have not verified.
    UnknownTargetRoot(Hash256),
    /// A signature on the attestation is invalid.
    InvalidSignature,
    /// There is no committee for the slot and committee index of this attestation and the
    /// attestation should not have been produced.
    NoCommitteeForSlotAndIndex {
        slot: Slot,
        index: CommitteeIndex,
    },
    /// The unaggregated attestation doesn't have only one aggregation bit set.
    NotExactlyOneAggregationBitSet(usize),
    PriorAttestationKnown {
        validator_index: u64,
        epoch: Epoch,
    },
    /// The attestation is for an epoch in the future (with respect to the gossip clock disparity).
    FutureEpoch {
        attestation_epoch: Epoch,
        current_epoch: Epoch,
    },
    /// The attestation is for an epoch in the past (with respect to the gossip clock disparity).
    PastEpoch {
        attestation_epoch: Epoch,
        current_epoch: Epoch,
    },
    /// The attestation is attesting to a state that is later than itself. (Viz., attesting to the
    /// future).
    AttestsToFutureBlock {
        block: Slot,
        attestation: Slot,
    },
    /// The attestation failed the `state_processing` verification stage.
    Invalid(AttestationValidationError),
    /// There was an error whilst processing the attestation. It is not known if it is valid or invalid.
    BeaconChainError(BeaconChainError),
}

impl From<BeaconChainError> for Error {
    fn from(e: BeaconChainError) -> Self {
        Error::BeaconChainError(e)
    }
}

/// Wraps a `SignedAggregateAndProof` that has been verified for propagation on the gossip network.
pub struct VerifiedAggregatedAttestation<T: BeaconChainTypes> {
    signed_aggregate: SignedAggregateAndProof<T::EthSpec>,
    indexed_attestation: IndexedAttestation<T::EthSpec>,
}

/// Wraps an `Attestation` that has been verified for propagation on the gossip network.
pub struct VerifiedUnaggregatedAttestation<T: BeaconChainTypes> {
    attestation: Attestation<T::EthSpec>,
    indexed_attestation: IndexedAttestation<T::EthSpec>,
}

/// Wraps an `indexed_attestation` that is valid for application to fork choice. The
/// `indexed_attestation` will have been generated via the `VerifiedAggregatedAttestation` or
/// `VerifiedUnaggregatedAttestation` wrappers.
pub struct ForkChoiceVerifiedAttestation<T: BeaconChainTypes> {
    indexed_attestation: IndexedAttestation<T::EthSpec>,
}

/// A helper trait implemented on wrapper types that can be progressed to a state where they can be
/// verified for application to fork choice.
pub trait IntoForkChoiceVerifiedAttestation<T: BeaconChainTypes> {
    fn into_fork_choice_verified_attestation(
        self,
        chain: &BeaconChain<T>,
    ) -> Result<ForkChoiceVerifiedAttestation<T>, Error>;
}

impl<T: BeaconChainTypes> IntoForkChoiceVerifiedAttestation<T>
    for VerifiedAggregatedAttestation<T>
{
    /// Progresses the `VerifiedAggregatedAttestation` to a stage where it is valid for application
    /// to the fork-choice rule (or not).
    fn into_fork_choice_verified_attestation(
        self,
        chain: &BeaconChain<T>,
    ) -> Result<ForkChoiceVerifiedAttestation<T>, Error> {
        ForkChoiceVerifiedAttestation::from_signature_verified_components(
            self.indexed_attestation,
            chain,
        )
    }
}

impl<T: BeaconChainTypes> IntoForkChoiceVerifiedAttestation<T>
    for VerifiedUnaggregatedAttestation<T>
{
    /// Progresses the `Attestation` to a stage where it is valid for application to the
    /// fork-choice rule (or not).
    fn into_fork_choice_verified_attestation(
        self,
        chain: &BeaconChain<T>,
    ) -> Result<ForkChoiceVerifiedAttestation<T>, Error> {
        ForkChoiceVerifiedAttestation::from_signature_verified_components(
            self.indexed_attestation,
            chain,
        )
    }
}

impl<T: BeaconChainTypes> IntoForkChoiceVerifiedAttestation<T>
    for ForkChoiceVerifiedAttestation<T>
{
    /// Simply returns itself.
    fn into_fork_choice_verified_attestation(
        self,
        _: &BeaconChain<T>,
    ) -> Result<ForkChoiceVerifiedAttestation<T>, Error> {
        Ok(self)
    }
}

impl<T: BeaconChainTypes> VerifiedAggregatedAttestation<T> {
    /// Returns `Ok(Self)` if the `signed_aggregate` is valid to be (re)published on the gossip
    /// network.
    pub fn verify(
        signed_aggregate: SignedAggregateAndProof<T::EthSpec>,
        chain: &BeaconChain<T>,
    ) -> Result<Self, Error> {
        let attestation = &signed_aggregate.message.aggregate;

        // Ensure attestation is within the last ATTESTATION_PROPAGATION_SLOT_RANGE slots (within a
        // MAXIMUM_GOSSIP_CLOCK_DISPARITY allowance).
        //
        // We do not queue future attestations for later processing.
        verify_propagation_slot_range(chain, attestation)?;

        // Ensure the aggregated attestation has not already been seen locally.
        //
        // TODO: this part of the code is not technically to spec, however I have raised a PR to
        // change it:
        //
        // https://github.com/ethereum/eth2.0-specs/pull/1749
        let attestation_root = attestation.tree_hash_root();
        if chain
            .observed_attestations
            .is_known(attestation, attestation_root)
            .map_err(|e| Error::BeaconChainError(e.into()))?
        {
            return Err(Error::AttestationAlreadyKnown(attestation_root));
        }

        let aggregator_index = signed_aggregate.message.aggregator_index;

        // Ensure there has been no other observed aggregate for the given `aggregator_index`.
        //
        // Note: do not observe yet, only observe once the attestation has been verfied.
        match chain
            .observed_aggregators
            .validator_has_been_observed(attestation, aggregator_index as usize)
        {
            Ok(true) => Err(Error::AggregatorAlreadyKnown(aggregator_index)),
            Ok(false) => Ok(()),
            Err(ObservedAttestersError::ValidatorIndexTooHigh(i)) => {
                Err(Error::ValidatorIndexTooHigh(i))
            }
            Err(e) => Err(BeaconChainError::from(e).into()),
        }?;

        // Ensure the block being voted for (attestation.data.beacon_block_root) passes validation.
        //
        // This indirectly checks to see if the `attestation.data.beacon_block_root` is in our fork
        // choice. Any known, non-finalized, processed block should be in fork choice, so this
        // check immediately filters out attestations that attest to a block that has not been
        // processed.
        //
        // Attestations must be for a known block. If the block is unknown, we simply drop the
        // attestation and do not delay consideration for later.
        verify_head_block_is_known(chain, &attestation)?;

        let indexed_attestation = map_attestation_committee(chain, attestation, |committee| {
            // Note: this clones the signature which is known to be a relatively slow operation.
            //
            // Future optimizations should remove this clone.
            let selection_proof =
                SelectionProof::from(signed_aggregate.message.selection_proof.clone());

            if !selection_proof
                .is_aggregator(committee.committee.len(), &chain.spec)
                .map_err(|e| Error::BeaconChainError(e.into()))?
            {
                return Err(Error::InvalidSelectionProof { aggregator_index });
            }

            if !committee
                .committee
                .iter()
                .any(|validator_index| *validator_index as u64 == aggregator_index)
            {
                return Err(Error::AggregatorNotInCommittee { aggregator_index });
            }

            get_indexed_attestation(committee.committee, &attestation)
                .map_err(|e| BeaconChainError::from(e).into())
        })?;

        if !verify_signed_aggregate_signatures(chain, &signed_aggregate, &indexed_attestation)? {
            return Err(Error::InvalidSignature);
        }

        // Observe the valid attestation so we do not re-process it.
        //
        // It's important to double check that the attestation is not already known, otherwise two
        // attestations processed at the same time could be published.
        if let ObserveOutcome::AlreadyKnown = chain
            .observed_attestations
            .observe_attestation(attestation, Some(attestation_root))
            .map_err(|e| Error::BeaconChainError(e.into()))?
        {
            return Err(Error::AttestationAlreadyKnown(attestation_root));
        }

        // Observe the aggregator so we don't process another aggregate from them.
        //
        // It's important to double check that the attestation is not already known, otherwise two
        // attestations processed at the same time could be published.
        if chain
            .observed_aggregators
            .observe_validator(&attestation, aggregator_index as usize)
            .map_err(|e| BeaconChainError::from(e))?
        {
            return Err(Error::PriorAttestationKnown {
                validator_index: aggregator_index,
                epoch: attestation.data.target.epoch,
            });
        }

        Ok(VerifiedAggregatedAttestation {
            signed_aggregate,
            indexed_attestation,
        })
    }

    /// A helper function to add this aggregate to `beacon_chain.op_pool`.
    pub fn add_to_pool(self, chain: &BeaconChain<T>) -> Result<Self, Error> {
        chain.add_to_block_inclusion_pool(self)
    }

    /// A helper function to add this aggregate to `beacon_chain.fork_choice`.
    pub fn add_to_fork_choice(
        self,
        chain: &BeaconChain<T>,
    ) -> Result<ForkChoiceVerifiedAttestation<T>, Error> {
        chain.apply_attestation_to_fork_choice(self)
    }

    /// Returns the underlying `attestation` for the `signed_aggregate`.
    pub fn attestation(&self) -> &Attestation<T::EthSpec> {
        &self.signed_aggregate.message.aggregate
    }
}

impl<T: BeaconChainTypes> VerifiedUnaggregatedAttestation<T> {
    /// Returns `Ok(Self)` if the `attestation` is valid to be (re)published on the gossip
    /// network.
    pub fn verify(
        attestation: Attestation<T::EthSpec>,
        chain: &BeaconChain<T>,
    ) -> Result<Self, Error> {
        // Ensure attestation is within the last ATTESTATION_PROPAGATION_SLOT_RANGE slots (within a
        // MAXIMUM_GOSSIP_CLOCK_DISPARITY allowance).
        //
        // We do not queue future attestations for later processing.
        verify_propagation_slot_range(chain, &attestation)?;

        // Check to ensure that the attestation is "unaggregated". I.e., it has exactly one
        // aggregation bit set.
        let num_aggreagtion_bits = attestation.aggregation_bits.num_set_bits();
        if num_aggreagtion_bits != 1 {
            return Err(Error::NotExactlyOneAggregationBitSet(num_aggreagtion_bits));
        }

        // Attestations must be for a known block. If the block is unknown, we simply drop the
        // attestation and do not delay consideration for later.
        verify_head_block_is_known(chain, &attestation)?;

        let indexed_attestation = obtain_indexed_attestation(chain, &attestation)?;

        let validator_index = *indexed_attestation
            .attesting_indices
            .first()
            .ok_or_else(|| Error::NotExactlyOneAggregationBitSet(0))?;

        /*
         * The attestation is the first valid attestation received for the participating validator
         * for the slot, attestation.data.slot.
         */
        if chain
            .observed_attesters
            .validator_has_been_observed(&attestation, validator_index as usize)
            .map_err(|e| BeaconChainError::from(e))?
        {
            return Err(Error::PriorAttestationKnown {
                validator_index,
                epoch: attestation.data.target.epoch,
            });
        }

        // The signature of attestation is valid.
        verify_attestation_signature(chain, &indexed_attestation)?;

        // Now that the attestation has been full verified, note that we have received a valid
        // attestation from this validator.
        //
        // It's important to double check that the attestation still hasn't been observed, since
        // there can be a race-condition if we receive two attestations at the same time and
        // process them in different threads.
        if chain
            .observed_attesters
            .observe_validator(&attestation, validator_index as usize)
            .map_err(|e| BeaconChainError::from(e))?
        {
            return Err(Error::PriorAttestationKnown {
                validator_index,
                epoch: attestation.data.target.epoch,
            });
        }

        Ok(Self {
            attestation,
            indexed_attestation,
        })
    }

    /// A helper function to add this attestation to `beacon_chain.op_pool`.
    pub fn add_to_pool(self, chain: &BeaconChain<T>) -> Result<Self, Error> {
        chain.add_to_naive_aggregation_pool(self)
    }

    /// Returns the wrapped `attestation`.
    pub fn attestation(&self) -> &Attestation<T::EthSpec> {
        &self.attestation
    }
}

impl<T: BeaconChainTypes> ForkChoiceVerifiedAttestation<T> {
    /// Returns `Ok(Self)` if the `attestation` is valid to be applied to the beacon chain fork
    /// choice.
    ///
    /// The supplied `indexed_attestation` MUST have a valid signature, this function WILL NOT
    /// CHECK THE SIGNATURE. Use the `VerifiedAggregatedAttestation` or
    /// `VerifiedUnaggregatedAttestation` structs to do signature verification.
    fn from_signature_verified_components(
        indexed_attestation: IndexedAttestation<T::EthSpec>,
        chain: &BeaconChain<T>,
    ) -> Result<Self, Error> {
        // There is no point in processing an attestation with an empty bitfield. Reject
        // it immediately.
        if indexed_attestation.attesting_indices.len() == 0 {
            return Err(Error::EmptyAggregationBitfield);
        }

        let attestation_epoch = indexed_attestation
            .data
            .slot
            .epoch(T::EthSpec::slots_per_epoch());
        let epoch_now = chain.epoch()?;
        let target = indexed_attestation.data.target.clone();

        // Attestation must be from the current or previous epoch.
        if attestation_epoch > epoch_now {
            return Err(Error::FutureEpoch {
                attestation_epoch,
                current_epoch: epoch_now,
            });
        } else if attestation_epoch + 1 < epoch_now {
            return Err(Error::PastEpoch {
                attestation_epoch,
                current_epoch: epoch_now,
            });
        }

        if target.epoch
            != indexed_attestation
                .data
                .slot
                .epoch(T::EthSpec::slots_per_epoch())
        {
            return Err(Error::BadTargetEpoch);
        }

        // Load the slot and state root for `attestation.data.beacon_block_root`.
        //
        // This indirectly checks to see if the `attestation.data.beacon_block_root` is in our fork
        // choice. Any known, non-finalized block should be in fork choice, so this check
        // immediately filters out attestations that attest to a block that has not been processed.
        //
        // Attestations must be for a known block. If the block is unknown, we simply drop the
        // attestation and do not delay consideration for later.
        let (block_slot, _state_root) = chain
            .fork_choice
            .block_slot_and_state_root(&indexed_attestation.data.beacon_block_root)
            .ok_or_else(|| Error::UnknownHeadBlock {
                beacon_block_root: indexed_attestation.data.beacon_block_root,
            })?;

        // TODO: currently we do not check the FFG source/target. This is what the spec dictates
        // but it seems wrong.
        //
        // I have opened an issue on the specs repo for this:
        //
        // https://github.com/ethereum/eth2.0-specs/issues/1636
        //
        // We should revisit this code once that issue has been resolved.

        // Attestations must not be for blocks in the future. If this is the case, the attestation
        // should not be considered.
        if block_slot > indexed_attestation.data.slot {
            return Err(Error::AttestsToFutureBlock {
                block: block_slot,
                attestation: indexed_attestation.data.slot,
            });
        }

        Ok(Self {
            indexed_attestation,
        })
    }

    /// Returns the wrapped `IndexedAttestation`.
    pub fn indexed_attestation(&self) -> &IndexedAttestation<T::EthSpec> {
        &self.indexed_attestation
    }
}

/// Returns `Ok(())` if the `attestation.data.beacon_block_root` is known to this chain.
///
/// The block root may not be known for two reasons:
///
/// 1. The block has never been verified by our application.
/// 2. The block is prior to the latest finalized block.
///
/// Case (1) is the exact thing we're trying to detect. However case (2) is a little different, but
/// it's still fine to reject here because there's no need for us to handle attestations that are
/// already finalized.
///
/// TODO: will we trigger a parent lookup if someone sends us a finalized block??
fn verify_head_block_is_known<T: BeaconChainTypes>(
    chain: &BeaconChain<T>,
    attestation: &Attestation<T::EthSpec>,
) -> Result<(), Error> {
    if chain
        .fork_choice
        .contains_block(&attestation.data.beacon_block_root)
    {
        Ok(())
    } else {
        Err(Error::UnknownHeadBlock {
            beacon_block_root: attestation.data.beacon_block_root,
        })
    }
}

/// Verify that the `attestation` is within the acceptable gossip propagation range, with reference
/// to the current slot of the `chain`.
///
/// Accounts for `MAXIMUM_GOSSIP_CLOCK_DISPARITY`>
pub fn verify_propagation_slot_range<T: BeaconChainTypes>(
    chain: &BeaconChain<T>,
    attestation: &Attestation<T::EthSpec>,
) -> Result<(), Error> {
    let attestation_slot = attestation.data.slot;

    let latest_permissible_slot = chain
        .slot_clock
        .now_with_future_tolerance(MAXIMUM_GOSSIP_CLOCK_DISPARITY)
        .ok_or_else(|| BeaconChainError::UnableToReadSlot)?;
    if attestation_slot > latest_permissible_slot {
        return Err(Error::FutureSlot {
            attestation_slot,
            latest_permissible_slot,
        });
    }

    // Taking advantage of saturating subtraction on `Slot`.
    let earliest_permissible_slot = chain
        .slot_clock
        .now_with_past_tolerance(MAXIMUM_GOSSIP_CLOCK_DISPARITY)
        .ok_or_else(|| BeaconChainError::UnableToReadSlot)?
        - T::EthSpec::slots_per_epoch();
    if attestation_slot < earliest_permissible_slot {
        return Err(Error::PastSlot {
            attestation_slot,
            earliest_permissible_slot,
        });
    }

    Ok(())
}

/// Verifies that the signature of the `indexed_attestation` is valid.
pub fn verify_attestation_signature<T: BeaconChainTypes>(
    chain: &BeaconChain<T>,
    indexed_attestation: &IndexedAttestation<T::EthSpec>,
) -> Result<(), Error> {
    let signature_setup_timer =
        metrics::start_timer(&metrics::ATTESTATION_PROCESSING_SIGNATURE_SETUP_TIMES);

    let pubkey_cache = chain
        .validator_pubkey_cache
        .try_read_for(VALIDATOR_PUBKEY_CACHE_LOCK_TIMEOUT)
        .ok_or_else(|| BeaconChainError::ValidatorPubkeyCacheLockTimeout)?;

    let fork = chain
        .canonical_head
        .try_read_for(HEAD_LOCK_TIMEOUT)
        .ok_or_else(|| BeaconChainError::CanonicalHeadLockTimeout)
        .map(|head| head.beacon_state.fork.clone())?;

    let signature_set = indexed_attestation_signature_set_from_pubkeys(
        |validator_index| {
            pubkey_cache
                .get(validator_index)
                .map(|pk| Cow::Borrowed(pk.as_point()))
        },
        &indexed_attestation.signature,
        &indexed_attestation,
        &fork,
        chain.genesis_validators_root,
        &chain.spec,
    )
    .map_err(BeaconChainError::SignatureSetError)?;

    metrics::stop_timer(signature_setup_timer);

    let _signature_verification_timer =
        metrics::start_timer(&metrics::ATTESTATION_PROCESSING_SIGNATURE_TIMES);

    if signature_set.is_valid() {
        Ok(())
    } else {
        Err(Error::InvalidSignature)
    }
}

/// Verifies all the signatures in a `SignedAggregateAndProof` using BLS batch verification. This
/// includes three signatures:
///
/// - `signed_aggregate.signature`
/// - `signed_aggregate.signature.message.selection proof`
/// - `signed_aggregate.signature.message.aggregate.signature`
///
/// # Returns
///
/// - `Ok(true)`: if all signatures are valid.
/// - `Ok(false)`: if one or more signatures are invalid.
/// - `Err(e)`: if there was an error preventing signature verification.
pub fn verify_signed_aggregate_signatures<T: BeaconChainTypes>(
    chain: &BeaconChain<T>,
    signed_aggregate: &SignedAggregateAndProof<T::EthSpec>,
    indexed_attestation: &IndexedAttestation<T::EthSpec>,
) -> Result<bool, Error> {
    let pubkey_cache = chain
        .validator_pubkey_cache
        .try_read_for(VALIDATOR_PUBKEY_CACHE_LOCK_TIMEOUT)
        .ok_or_else(|| BeaconChainError::ValidatorPubkeyCacheLockTimeout)?;

    let aggregator_index = signed_aggregate.message.aggregator_index;
    if aggregator_index >= pubkey_cache.len() as u64 {
        return Err(Error::AggregatorPubkeyUnknown(aggregator_index));
    }

    let fork = chain
        .canonical_head
        .try_read_for(HEAD_LOCK_TIMEOUT)
        .ok_or_else(|| BeaconChainError::CanonicalHeadLockTimeout)
        .map(|head| head.beacon_state.fork.clone())?;

    let signature_sets = vec![
        signed_aggregate_selection_proof_signature_set(
            |validator_index| {
                pubkey_cache
                    .get(validator_index)
                    .map(|pk| Cow::Borrowed(pk.as_point()))
            },
            &signed_aggregate,
            &fork,
            chain.genesis_validators_root,
            &chain.spec,
        )
        .map_err(BeaconChainError::SignatureSetError)?,
        signed_aggregate_signature_set(
            |validator_index| {
                pubkey_cache
                    .get(validator_index)
                    .map(|pk| Cow::Borrowed(pk.as_point()))
            },
            &signed_aggregate,
            &fork,
            chain.genesis_validators_root,
            &chain.spec,
        )
        .map_err(BeaconChainError::SignatureSetError)?,
        indexed_attestation_signature_set_from_pubkeys(
            |validator_index| {
                pubkey_cache
                    .get(validator_index)
                    .map(|pk| Cow::Borrowed(pk.as_point()))
            },
            &indexed_attestation.signature,
            &indexed_attestation,
            &fork,
            chain.genesis_validators_root,
            &chain.spec,
        )
        .map_err(BeaconChainError::SignatureSetError)?,
    ];

    Ok(verify_signature_sets(signature_sets.into_iter()))
}

/// Returns the `indexed_attestation` for the `attestation` using the public keys cached in the
/// `chain`.
pub fn obtain_indexed_attestation<T: BeaconChainTypes>(
    chain: &BeaconChain<T>,
    attestation: &Attestation<T::EthSpec>,
) -> Result<IndexedAttestation<T::EthSpec>, Error> {
    map_attestation_committee(chain, attestation, |committee| {
        get_indexed_attestation(committee.committee, &attestation)
            .map_err(|e| BeaconChainError::from(e).into())
    })
}

/// Runs the `map_fn` with the committee for the given `attestation`.
///
/// This function exists in this odd "map" pattern because efficiently obtaining the committee for
/// an attestation can be complex. It might involve reading straight from the
/// `beacon_chain.shuffling_cache` or it might involve reading it from a state from the DB. Due to
/// the complexities of `RwLock`s on the shuffling cache, a simple `Cow` isn't suitable here.
pub fn map_attestation_committee<'a, T, F, R>(
    chain: &'a BeaconChain<T>,
    attestation: &Attestation<T::EthSpec>,
    map_fn: F,
) -> Result<R, Error>
where
    T: BeaconChainTypes,
    F: Fn(BeaconCommittee) -> Result<R, Error>,
{
    let attestation_epoch = attestation.data.slot.epoch(T::EthSpec::slots_per_epoch());
    let target = &attestation.data.target;

    // Attestation target must be for a known block.
    //
    // We use fork choice to find the target root, which means that we reject any attestation
    // that has a `target.root` earlier than our latest finalized root. There's no point in
    // processing an attestation that does not include our latest finalized block in its chain.
    //
    // We do not delay consideration for later, we simply drop the attestation.
    //
    // TODO: make sure this isn't too strict....
    let (target_block_slot, target_block_state_root) = chain
        .fork_choice
        .block_slot_and_state_root(&target.root)
        .ok_or_else(|| Error::UnknownTargetRoot(target.root))?;

    // Obtain the shuffling cache, timing how long we wait.
    let cache_wait_timer =
        metrics::start_timer(&metrics::ATTESTATION_PROCESSING_SHUFFLING_CACHE_WAIT_TIMES);

    let mut shuffling_cache = chain
        .shuffling_cache
        .try_write_for(ATTESTATION_CACHE_LOCK_TIMEOUT)
        .ok_or_else(|| BeaconChainError::AttestationCacheLockTimeout)?;

    metrics::stop_timer(cache_wait_timer);

    if let Some(committee_cache) = shuffling_cache.get(attestation_epoch, target.root) {
        committee_cache
            .get_beacon_committee(attestation.data.slot, attestation.data.index)
            .map(map_fn)
            .unwrap_or_else(|| {
                Err(Error::NoCommitteeForSlotAndIndex {
                    slot: attestation.data.slot,
                    index: attestation.data.index,
                })
            })
    } else {
        // Drop the shuffling cache to avoid holding the lock for any longer than
        // required.
        drop(shuffling_cache);

        debug!(
            chain.log,
            "Attestation processing cache miss";
            "attn_epoch" => attestation_epoch.as_u64(),
            "target_block_epoch" => target_block_slot.epoch(T::EthSpec::slots_per_epoch()).as_u64(),
        );

        let state_read_timer =
            metrics::start_timer(&metrics::ATTESTATION_PROCESSING_STATE_READ_TIMES);

        let mut state = chain
            .get_state(&target_block_state_root, Some(target_block_slot))?
            .ok_or_else(|| BeaconChainError::MissingBeaconState(target_block_state_root))?;

        metrics::stop_timer(state_read_timer);
        let state_skip_timer =
            metrics::start_timer(&metrics::ATTESTATION_PROCESSING_STATE_SKIP_TIMES);

        while state.current_epoch() + 1 < attestation_epoch {
            // Here we tell `per_slot_processing` to skip hashing the state and just
            // use the zero hash instead.
            //
            // The state roots are not useful for the shuffling, so there's no need to
            // compute them.
            per_slot_processing(&mut state, Some(Hash256::zero()), &chain.spec)
                .map_err(|e| BeaconChainError::from(e))?
        }

        metrics::stop_timer(state_skip_timer);
        let committee_building_timer =
            metrics::start_timer(&metrics::ATTESTATION_PROCESSING_COMMITTEE_BUILDING_TIMES);

        let relative_epoch = RelativeEpoch::from_epoch(state.current_epoch(), attestation_epoch)
            .map_err(BeaconChainError::IncorrectStateForAttestation)?;

        state
            .build_committee_cache(relative_epoch, &chain.spec)
            .map_err(|e| BeaconChainError::from(e))?;

        let committee_cache = state
            .committee_cache(relative_epoch)
            .map_err(|e| BeaconChainError::from(e))?;

        chain
            .shuffling_cache
            .try_write_for(ATTESTATION_CACHE_LOCK_TIMEOUT)
            .ok_or_else(|| BeaconChainError::AttestationCacheLockTimeout)?
            .insert(attestation_epoch, target.root, committee_cache);

        metrics::stop_timer(committee_building_timer);

        committee_cache
            .get_beacon_committee(attestation.data.slot, attestation.data.index)
            .map(map_fn)
            .unwrap_or_else(|| {
                Err(Error::NoCommitteeForSlotAndIndex {
                    slot: attestation.data.slot,
                    index: attestation.data.index,
                })
            })
    }
}
