use super::{
    Attestation, ChainSpec, Domain, EthSpec, Fork, Hash256, PublicKey, SecretKey, SelectionProof,
    Signature, SignedRoot,
};
use crate::test_utils::TestRandom;
use serde_derive::{Deserialize, Serialize};
use ssz_derive::{Decode, Encode};
use test_random_derive::TestRandom;
use tree_hash_derive::TreeHash;

/// A Validators aggregate attestation and selection proof.
///
/// Spec v0.10.1
#[cfg_attr(feature = "arbitrary-fuzz", derive(arbitrary::Arbitrary))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Encode, Decode, TestRandom, TreeHash)]
#[serde(bound = "T: EthSpec")]
pub struct AggregateAndProof<T: EthSpec> {
    /// The index of the validator that created the attestation.
    pub aggregator_index: u64,
    /// The aggregate attestation.
    pub aggregate: Attestation<T>,
    /// A proof provided by the validator that permits them to publish on the
    /// `beacon_aggregate_and_proof` gossipsub topic.
    pub selection_proof: Signature,
}

impl<T: EthSpec> AggregateAndProof<T> {
    /// Produces a new `AggregateAndProof` with a `selection_proof` generated by signing
    /// `aggregate.data.slot` with `secret_key`.
    ///
    /// If `selection_proof.is_none()` it will be computed locally.
    pub fn from_aggregate(
        aggregator_index: u64,
        aggregate: Attestation<T>,
        selection_proof: Option<SelectionProof>,
        secret_key: &SecretKey,
        fork: &Fork,
        genesis_validators_root: Hash256,
        spec: &ChainSpec,
    ) -> Self {
        let selection_proof = selection_proof
            .unwrap_or_else(|| {
                SelectionProof::new::<T>(
                    aggregate.data.slot,
                    secret_key,
                    fork,
                    genesis_validators_root,
                    spec,
                )
            })
            .into();

        Self {
            aggregator_index,
            aggregate,
            selection_proof,
        }
    }

    /// Returns `true` if `validator_pubkey` signed over `self.aggregate.data.slot`.
    pub fn is_valid_selection_proof(
        &self,
        validator_pubkey: &PublicKey,
        fork: &Fork,
        genesis_validators_root: Hash256,
        spec: &ChainSpec,
    ) -> bool {
        let target_epoch = self.aggregate.data.slot.epoch(T::slots_per_epoch());
        let domain = spec.get_domain(
            target_epoch,
            Domain::SelectionProof,
            fork,
            genesis_validators_root,
        );
        let message = self.aggregate.data.slot.signing_root(domain);
        self.selection_proof
            .verify(message.as_bytes(), validator_pubkey)
    }
}

impl<T: EthSpec> SignedRoot for AggregateAndProof<T> {}
