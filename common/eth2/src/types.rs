//! This module exposes a superset of the `types` crate. It adds additional types that are only
//! required for the HTTP API.

use eth2_libp2p::{Enr, Multiaddr};
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::fmt;
use std::str::FromStr;

pub use types::*;

/// An API error serializable to JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorMessage {
    pub code: u16,
    pub message: String,
    #[serde(default)]
    pub stacktraces: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenesisData {
    #[serde(with = "serde_utils::quoted_u64")]
    pub genesis_time: u64,
    pub genesis_validators_root: Hash256,
    #[serde(with = "serde_utils::bytes_4_hex")]
    pub genesis_fork_version: [u8; 4],
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum BlockId {
    Head,
    Genesis,
    Finalized,
    Justified,
    Slot(Slot),
    Root(Hash256),
}

impl FromStr for BlockId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "head" => Ok(BlockId::Head),
            "genesis" => Ok(BlockId::Genesis),
            "finalized" => Ok(BlockId::Finalized),
            "justified" => Ok(BlockId::Justified),
            other => {
                if other.starts_with("0x") {
                    Hash256::from_str(&s[2..])
                        .map(BlockId::Root)
                        .map_err(|e| format!("{} cannot be parsed as a root", e))
                } else {
                    u64::from_str(s)
                        .map(Slot::new)
                        .map(BlockId::Slot)
                        .map_err(|_| format!("{} cannot be parsed as a parameter", s))
                }
            }
        }
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockId::Head => write!(f, "head"),
            BlockId::Genesis => write!(f, "genesis"),
            BlockId::Finalized => write!(f, "finalized"),
            BlockId::Justified => write!(f, "justified"),
            BlockId::Slot(slot) => write!(f, "{}", slot),
            BlockId::Root(root) => write!(f, "{:?}", root),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum StateId {
    Head,
    Genesis,
    Finalized,
    Justified,
    Slot(Slot),
    Root(Hash256),
}

impl FromStr for StateId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "head" => Ok(StateId::Head),
            "genesis" => Ok(StateId::Genesis),
            "finalized" => Ok(StateId::Finalized),
            "justified" => Ok(StateId::Justified),
            other => {
                if other.starts_with("0x") {
                    Hash256::from_str(&s[2..])
                        .map(StateId::Root)
                        .map_err(|e| format!("{} cannot be parsed as a root", e))
                } else {
                    u64::from_str(s)
                        .map(Slot::new)
                        .map(StateId::Slot)
                        .map_err(|_| format!("{} cannot be parsed as a slot", s))
                }
            }
        }
    }
}

impl fmt::Display for StateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateId::Head => write!(f, "head"),
            StateId::Genesis => write!(f, "genesis"),
            StateId::Finalized => write!(f, "finalized"),
            StateId::Justified => write!(f, "justified"),
            StateId::Slot(slot) => write!(f, "{}", slot),
            StateId::Root(root) => write!(f, "{:?}", root),
        }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
#[serde(bound = "T: Serialize + serde::de::DeserializeOwned")]
pub struct GenericResponse<T: Serialize + serde::de::DeserializeOwned> {
    pub data: T,
}

impl<T: Serialize + serde::de::DeserializeOwned> From<T> for GenericResponse<T> {
    fn from(data: T) -> Self {
        Self { data }
    }
}

#[derive(Debug, PartialEq, Clone, Serialize)]
#[serde(bound = "T: Serialize")]
pub struct GenericResponseRef<'a, T: Serialize> {
    pub data: &'a T,
}

impl<'a, T: Serialize> From<&'a T> for GenericResponseRef<'a, T> {
    fn from(data: &'a T) -> Self {
        Self { data }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RootData {
    pub root: Hash256,
}

impl From<Hash256> for RootData {
    fn from(root: Hash256) -> Self {
        Self { root }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FinalityCheckpointsData {
    pub previous_justified: Checkpoint,
    pub current_justified: Checkpoint,
    pub finalized: Checkpoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ValidatorId {
    PublicKey(PublicKeyBytes),
    Index(u64),
}

impl FromStr for ValidatorId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with("0x") {
            PublicKeyBytes::from_str(s)
                .map(ValidatorId::PublicKey)
                .map_err(|e| format!("{} cannot be parsed as a public key: {}", s, e))
        } else {
            u64::from_str(s)
                .map(ValidatorId::Index)
                .map_err(|e| format!("{} cannot be parsed as a slot: {}", s, e))
        }
    }
}

impl fmt::Display for ValidatorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidatorId::PublicKey(pubkey) => write!(f, "{:?}", pubkey),
            ValidatorId::Index(index) => write!(f, "{}", index),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidatorData {
    #[serde(with = "serde_utils::quoted_u64")]
    pub index: u64,
    #[serde(with = "serde_utils::quoted_u64")]
    pub balance: u64,
    pub status: ValidatorStatus,
    pub validator: Validator,
}

// TODO: This does not currently match the spec, but I'm going to try and change the spec using
// this proposal:
//
// https://hackmd.io/bQxMDRt1RbS1TLno8K4NPg?view
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValidatorStatus {
    Unknown,
    WaitingForEligibility,
    WaitingForFinality,
    WaitingInQueue,
    StandbyForActive(Epoch),
    Active,
    ActiveAwaitingVoluntaryExit(Epoch),
    ActiveAwaitingSlashedExit(Epoch),
    ExitedVoluntarily(Epoch),
    ExitedSlashed(Epoch),
    Withdrawable,
    Withdrawn,
}

impl ValidatorStatus {
    pub fn from_validator(
        validator_opt: Option<&Validator>,
        epoch: Epoch,
        finalized_epoch: Epoch,
        far_future_epoch: Epoch,
    ) -> Self {
        if let Some(validator) = validator_opt {
            if validator.is_withdrawable_at(epoch) {
                ValidatorStatus::Withdrawable
            } else if validator.is_exited_at(epoch) {
                if validator.slashed {
                    ValidatorStatus::ExitedSlashed(validator.withdrawable_epoch)
                } else {
                    ValidatorStatus::ExitedVoluntarily(validator.withdrawable_epoch)
                }
            } else if validator.is_active_at(epoch) {
                if validator.exit_epoch < far_future_epoch {
                    if validator.slashed {
                        ValidatorStatus::ActiveAwaitingSlashedExit(validator.exit_epoch)
                    } else {
                        ValidatorStatus::ActiveAwaitingVoluntaryExit(validator.exit_epoch)
                    }
                } else {
                    ValidatorStatus::Active
                }
            } else if validator.activation_epoch < far_future_epoch {
                ValidatorStatus::StandbyForActive(validator.activation_epoch)
            } else if validator.activation_eligibility_epoch < far_future_epoch {
                if finalized_epoch < validator.activation_eligibility_epoch {
                    ValidatorStatus::WaitingForFinality
                } else {
                    ValidatorStatus::WaitingInQueue
                }
            } else {
                ValidatorStatus::WaitingForEligibility
            }
        } else {
            ValidatorStatus::Unknown
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct CommitteesQuery {
    pub slot: Option<Slot>,
    pub index: Option<u64>,
}

#[derive(Serialize, Deserialize)]
pub struct ValidatorsQuery {
    pub id: Option<Vec<ValidatorId>>,
    pub status: Option<Vec<ValidatorStatus>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommitteeData {
    #[serde(with = "serde_utils::quoted_u64")]
    pub index: u64,
    pub slot: Slot,
    #[serde(with = "serde_utils::quoted_u64_vec")]
    pub validators: Vec<u64>,
}

#[derive(Serialize, Deserialize)]
pub struct HeadersQuery {
    pub slot: Option<Slot>,
    pub parent_root: Option<Hash256>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockHeaderAndSignature {
    pub message: BeaconBlockHeader,
    pub signature: SignatureBytes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockHeaderData {
    pub root: Hash256,
    pub canonical: bool,
    pub header: BlockHeaderAndSignature,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DepositContractData {
    #[serde(with = "serde_utils::quoted_u64")]
    pub chain_id: u64,
    pub address: Address,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainHeadData {
    pub slot: Slot,
    pub root: Hash256,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdentityData {
    pub peer_id: String,
    pub enr: Enr,
    pub p2p_addresses: Vec<Multiaddr>,
    // TODO: missing the following fields:
    //
    // - discovery_addresses
    // - metadata
    //
    // Tracked here: https://github.com/sigp/lighthouse/issues/1434
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionData {
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncingData {
    pub is_syncing: bool,
    pub head_slot: Slot,
    pub sync_distance: Slot,
}

#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(try_from = "String", bound = "T: FromStr")]
pub struct QueryVec<T: FromStr>(pub Vec<T>);

impl<T: FromStr> TryFrom<String> for QueryVec<T> {
    type Error = String;

    fn try_from(string: String) -> Result<Self, Self::Error> {
        if string == "" {
            return Ok(Self(vec![]));
        }

        string
            .split(',')
            .map(|s| s.parse().map_err(|_| "unable to parse".to_string()))
            .collect::<Result<Vec<T>, String>>()
            .map(Self)
    }
}

#[derive(Clone, Deserialize)]
pub struct ValidatorDutiesQuery {
    pub index: Option<QueryVec<u64>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttesterData {
    pub pubkey: PublicKeyBytes,
    #[serde(with = "serde_utils::quoted_u64")]
    pub validator_index: u64,
    #[serde(with = "serde_utils::quoted_u64")]
    pub committees_at_slot: u64,
    #[serde(with = "serde_utils::quoted_u64")]
    pub committee_index: CommitteeIndex,
    #[serde(with = "serde_utils::quoted_u64")]
    pub committee_length: u64,
    #[serde(with = "serde_utils::quoted_u64")]
    pub validator_committee_index: u64,
    pub slot: Slot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposerData {
    pub pubkey: PublicKeyBytes,
    pub slot: Slot,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ValidatorBlocksQuery {
    pub randao_reveal: SignatureBytes,
    pub graffiti: Option<Graffiti>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ValidatorAttestationDataQuery {
    pub slot: Slot,
    pub committee_index: CommitteeIndex,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ValidatorAggregateAttestationQuery {
    pub attestation_data_root: Hash256,
    pub slot: Slot,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BeaconCommitteeSubscription {
    #[serde(with = "serde_utils::quoted_u64")]
    pub validator_index: u64,
    #[serde(with = "serde_utils::quoted_u64")]
    pub committee_index: u64,
    #[serde(with = "serde_utils::quoted_u64")]
    pub committees_at_slot: u64,
    pub slot: Slot,
    pub is_aggregator: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_vec() {
        assert_eq!(
            QueryVec::try_from("0,1,2".to_string()).unwrap(),
            QueryVec(vec![0_u64, 1, 2])
        );
    }
}
