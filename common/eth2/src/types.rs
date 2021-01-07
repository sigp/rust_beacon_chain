//! This module exposes a superset of the `types` crate. It adds additional types that are only
//! required for the HTTP API.

use crate::Error as ServerError;
use eth2_libp2p::{ConnectionDirection, Enr, Multiaddr, PeerConnectionStatus};
pub use reqwest::header::ACCEPT;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;
use std::fmt;
use std::str::{from_utf8, FromStr};
pub use types::*;

/// An API error serializable to JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorMessage {
    pub code: u16,
    pub message: String,
    #[serde(default)]
    pub stacktraces: Vec<String>,
}

/// An indexed API error serializable to JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IndexedErrorMessage {
    pub code: u16,
    pub message: String,
    pub failures: Vec<Failure>,
}

/// A single failure in an index of API errors, serializable to JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Failure {
    pub index: u64,
    pub message: String,
}

impl Failure {
    pub fn new(index: usize, message: String) -> Self {
        Self {
            index: index as u64,
            message,
        }
    }
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
pub struct DutiesResponse<T: Serialize + serde::de::DeserializeOwned> {
    pub dependent_root: Hash256,
    pub data: T,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidatorBalanceData {
    #[serde(with = "serde_utils::quoted_u64")]
    pub index: u64,
    #[serde(with = "serde_utils::quoted_u64")]
    pub balance: u64,
}

// Implemented according to what is described here:
//
// https://hackmd.io/ofFJ5gOmQpu1jjHilHbdQQ
//
// We expect this to be updated in v2 of the standard api to
// this proposal:
//
// https://hackmd.io/bQxMDRt1RbS1TLno8K4NPg?view
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorStatus {
    PendingInitialized,
    PendingQueued,
    ActiveOngoing,
    ActiveExiting,
    ActiveSlashed,
    ExitedUnslashed,
    ExitedSlashed,
    WithdrawalPossible,
    WithdrawalDone,
    Active,
    Pending,
    Exited,
    Withdrawal,
}

impl ValidatorStatus {
    pub fn from_validator(validator: &Validator, epoch: Epoch, far_future_epoch: Epoch) -> Self {
        if validator.is_withdrawable_at(epoch) {
            if validator.effective_balance == 0 {
                ValidatorStatus::WithdrawalDone
            } else {
                ValidatorStatus::WithdrawalPossible
            }
        } else if validator.is_exited_at(epoch) && epoch < validator.withdrawable_epoch {
            if validator.slashed {
                ValidatorStatus::ExitedSlashed
            } else {
                ValidatorStatus::ExitedUnslashed
            }
        } else if validator.is_active_at(epoch) {
            if validator.exit_epoch < far_future_epoch {
                if validator.slashed {
                    ValidatorStatus::ActiveSlashed
                } else {
                    ValidatorStatus::ActiveExiting
                }
            } else {
                ValidatorStatus::ActiveOngoing
            }
        // `pending` statuses are specified as validators where `validator.activation_epoch > current_epoch`.
        // If this code is reached, this criteria must have been met because `validator.is_active_at(epoch)`,
        // `validator.is_exited_at(epoch)`, and `validator.is_withdrawable_at(epoch)` all returned false.
        } else if validator.activation_eligibility_epoch == far_future_epoch {
            ValidatorStatus::PendingInitialized
        } else {
            ValidatorStatus::PendingQueued
        }
    }

    pub fn superstatus(&self) -> Self {
        match self {
            ValidatorStatus::PendingInitialized | ValidatorStatus::PendingQueued => {
                ValidatorStatus::Pending
            }
            ValidatorStatus::ActiveOngoing
            | ValidatorStatus::ActiveExiting
            | ValidatorStatus::ActiveSlashed => ValidatorStatus::Active,
            ValidatorStatus::ExitedUnslashed | ValidatorStatus::ExitedSlashed => {
                ValidatorStatus::Exited
            }
            ValidatorStatus::WithdrawalPossible | ValidatorStatus::WithdrawalDone => {
                ValidatorStatus::Withdrawal
            }
            ValidatorStatus::Active
            | ValidatorStatus::Pending
            | ValidatorStatus::Exited
            | ValidatorStatus::Withdrawal => *self,
        }
    }
}

impl FromStr for ValidatorStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending_initialized" => Ok(ValidatorStatus::PendingInitialized),
            "pending_queued" => Ok(ValidatorStatus::PendingQueued),
            "active_ongoing" => Ok(ValidatorStatus::ActiveOngoing),
            "active_exiting" => Ok(ValidatorStatus::ActiveExiting),
            "active_slashed" => Ok(ValidatorStatus::ActiveSlashed),
            "exited_unslashed" => Ok(ValidatorStatus::ExitedUnslashed),
            "exited_slashed" => Ok(ValidatorStatus::ExitedSlashed),
            "withdrawal_possible" => Ok(ValidatorStatus::WithdrawalPossible),
            "withdrawal_done" => Ok(ValidatorStatus::WithdrawalDone),
            "active" => Ok(ValidatorStatus::Active),
            "pending" => Ok(ValidatorStatus::Pending),
            "exited" => Ok(ValidatorStatus::Exited),
            "Withdrawal" => Ok(ValidatorStatus::Withdrawal),
            _ => Err(format!("{} cannot be parsed as a validator status.", s)),
        }
    }
}

impl fmt::Display for ValidatorStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidatorStatus::PendingInitialized => write!(f, "pending_initialized"),
            ValidatorStatus::PendingQueued => write!(f, "pending_queued"),
            ValidatorStatus::ActiveOngoing => write!(f, "active_ongoing"),
            ValidatorStatus::ActiveExiting => write!(f, "active_exiting"),
            ValidatorStatus::ActiveSlashed => write!(f, "active_slashed"),
            ValidatorStatus::ExitedUnslashed => write!(f, "exited_unslashed"),
            ValidatorStatus::ExitedSlashed => write!(f, "exited_slashed"),
            ValidatorStatus::WithdrawalPossible => write!(f, "withdrawal_possible"),
            ValidatorStatus::WithdrawalDone => write!(f, "withdrawal_done"),
            ValidatorStatus::Active => write!(f, "active"),
            ValidatorStatus::Pending => write!(f, "pending"),
            ValidatorStatus::Exited => write!(f, "exited"),
            ValidatorStatus::Withdrawal => write!(f, "withdrawal"),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct CommitteesQuery {
    pub slot: Option<Slot>,
    pub index: Option<u64>,
    pub epoch: Option<Epoch>,
}

#[derive(Serialize, Deserialize)]
pub struct AttestationPoolQuery {
    pub slot: Option<Slot>,
    pub committee_index: Option<u64>,
}

#[derive(Deserialize)]
pub struct ValidatorsQuery {
    pub id: Option<QueryVec<ValidatorId>>,
    pub status: Option<QueryVec<ValidatorStatus>>,
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
    pub discovery_addresses: Vec<Multiaddr>,
    pub metadata: MetaData,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetaData {
    #[serde(with = "serde_utils::quoted_u64")]
    pub seq_number: u64,
    pub attnets: String,
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
pub struct ValidatorBalancesQuery {
    pub id: Option<QueryVec<ValidatorId>>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ValidatorIndexData(#[serde(with = "serde_utils::quoted_u64_vec")] pub Vec<u64>);

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
    #[serde(with = "serde_utils::quoted_u64")]
    pub validator_index: u64,
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

#[derive(Deserialize)]
pub struct PeersQuery {
    pub state: Option<QueryVec<PeerState>>,
    pub direction: Option<QueryVec<PeerDirection>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerData {
    pub peer_id: String,
    pub enr: Option<String>,
    pub last_seen_p2p_address: String,
    pub state: PeerState,
    pub direction: PeerDirection,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeersData {
    pub data: Vec<PeerData>,
    pub meta: PeersMetaData,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeersMetaData {
    pub count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PeerState {
    Connected,
    Connecting,
    Disconnected,
    Disconnecting,
}

impl PeerState {
    pub fn from_peer_connection_status(status: &PeerConnectionStatus) -> Self {
        match status {
            PeerConnectionStatus::Connected { .. } => PeerState::Connected,
            PeerConnectionStatus::Dialing { .. } => PeerState::Connecting,
            PeerConnectionStatus::Disconnecting { .. } => PeerState::Disconnecting,
            PeerConnectionStatus::Disconnected { .. }
            | PeerConnectionStatus::Banned { .. }
            | PeerConnectionStatus::Unknown => PeerState::Disconnected,
        }
    }
}

impl FromStr for PeerState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "connected" => Ok(PeerState::Connected),
            "connecting" => Ok(PeerState::Connecting),
            "disconnected" => Ok(PeerState::Disconnected),
            "disconnecting" => Ok(PeerState::Disconnecting),
            _ => Err("peer state cannot be parsed.".to_string()),
        }
    }
}

impl fmt::Display for PeerState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PeerState::Connected => write!(f, "connected"),
            PeerState::Connecting => write!(f, "connecting"),
            PeerState::Disconnected => write!(f, "disconnected"),
            PeerState::Disconnecting => write!(f, "disconnecting"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PeerDirection {
    Inbound,
    Outbound,
}

impl PeerDirection {
    pub fn from_connection_direction(direction: &ConnectionDirection) -> Self {
        match direction {
            ConnectionDirection::Incoming => PeerDirection::Inbound,
            ConnectionDirection::Outgoing => PeerDirection::Outbound,
        }
    }
}

impl FromStr for PeerDirection {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "inbound" => Ok(PeerDirection::Inbound),
            "outbound" => Ok(PeerDirection::Outbound),
            _ => Err("peer direction cannot be parsed.".to_string()),
        }
    }
}

impl fmt::Display for PeerDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PeerDirection::Inbound => write!(f, "inbound"),
            PeerDirection::Outbound => write!(f, "outbound"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PeerCount {
    #[serde(with = "serde_utils::quoted_u64")]
    pub connected: u64,
    #[serde(with = "serde_utils::quoted_u64")]
    pub connecting: u64,
    #[serde(with = "serde_utils::quoted_u64")]
    pub disconnected: u64,
    #[serde(with = "serde_utils::quoted_u64")]
    pub disconnecting: u64,
}

// --------- Server Sent Event Types -----------

#[derive(PartialEq, Debug, Serialize, Deserialize, Clone)]
pub struct SseBlock {
    pub slot: Slot,
    pub block: Hash256,
}

#[derive(PartialEq, Debug, Serialize, Deserialize, Clone)]
pub struct SseFinalizedCheckpoint {
    pub block: Hash256,
    pub state: Hash256,
    pub epoch: Epoch,
}

#[derive(PartialEq, Debug, Serialize, Deserialize, Clone)]
pub struct SseHead {
    pub slot: Slot,
    pub block: Hash256,
    pub state: Hash256,
    pub current_duty_dependent_root: Hash256,
    pub previous_duty_dependent_root: Hash256,
    pub epoch_transition: bool,
}

#[derive(PartialEq, Debug, Serialize, Clone)]
#[serde(bound = "T: EthSpec", untagged)]
pub enum EventKind<T: EthSpec> {
    Attestation(Attestation<T>),
    Block(SseBlock),
    FinalizedCheckpoint(SseFinalizedCheckpoint),
    Head(SseHead),
    VoluntaryExit(SignedVoluntaryExit),
}

impl<T: EthSpec> EventKind<T> {
    pub fn from_sse_bytes(message: &[u8]) -> Result<Self, ServerError> {
        let s = from_utf8(message)
            .map_err(|e| ServerError::InvalidServerSentEvent(format!("{:?}", e)))?;

        let mut split = s.split('\n');
        let event = split
            .next()
            .ok_or_else(|| {
                ServerError::InvalidServerSentEvent("Could not parse event tag".to_string())
            })?
            .trim_start_matches("event:");
        let data = split
            .next()
            .ok_or_else(|| {
                ServerError::InvalidServerSentEvent("Could not parse data tag".to_string())
            })?
            .trim_start_matches("data:");

        match event {
            "attestation" => Ok(EventKind::Attestation(serde_json::from_str(data).map_err(
                |e| ServerError::InvalidServerSentEvent(format!("Attestation: {:?}", e)),
            )?)),
            "block" => Ok(EventKind::Block(serde_json::from_str(data).map_err(
                |e| ServerError::InvalidServerSentEvent(format!("Block: {:?}", e)),
            )?)),
            "finalized_checkpoint" => Ok(EventKind::FinalizedCheckpoint(
                serde_json::from_str(data).map_err(|e| {
                    ServerError::InvalidServerSentEvent(format!("Finalized Checkpoint: {:?}", e))
                })?,
            )),
            "head" => Ok(EventKind::Head(serde_json::from_str(data).map_err(
                |e| ServerError::InvalidServerSentEvent(format!("Head: {:?}", e)),
            )?)),
            "voluntary_exit" => Ok(EventKind::VoluntaryExit(
                serde_json::from_str(data).map_err(|e| {
                    ServerError::InvalidServerSentEvent(format!("Voluntary Exit: {:?}", e))
                })?,
            )),
            _ => Err(ServerError::InvalidServerSentEvent(
                "Could not parse event tag".to_string(),
            )),
        }
    }
}

#[derive(Clone, Deserialize)]
pub struct EventQuery {
    pub topics: QueryVec<EventTopic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventTopic {
    Head,
    Block,
    Attestation,
    VoluntaryExit,
    FinalizedCheckpoint,
}

impl FromStr for EventTopic {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "head" => Ok(EventTopic::Head),
            "block" => Ok(EventTopic::Block),
            "attestation" => Ok(EventTopic::Attestation),
            "voluntary_exit" => Ok(EventTopic::VoluntaryExit),
            "finalized_checkpoint" => Ok(EventTopic::FinalizedCheckpoint),
            _ => Err("event topic cannot be parsed.".to_string()),
        }
    }
}

impl fmt::Display for EventTopic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventTopic::Head => write!(f, "head"),
            EventTopic::Block => write!(f, "block"),
            EventTopic::Attestation => write!(f, "attestation"),
            EventTopic::VoluntaryExit => write!(f, "voluntary_exit"),
            EventTopic::FinalizedCheckpoint => write!(f, "finalized_checkpoint"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Accept {
    Json,
    Ssz,
    Any,
}

impl fmt::Display for Accept {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Accept::Ssz => write!(f, "application/octet-stream"),
            Accept::Json => write!(f, "application/json"),
            Accept::Any => write!(f, "*/*"),
        }
    }
}

impl FromStr for Accept {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "application/octet-stream" => Ok(Accept::Ssz),
            "application/json" => Ok(Accept::Json),
            "*/*" => Ok(Accept::Any),
            _ => Err("accept header cannot be parsed.".to_string()),
        }
    }
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
