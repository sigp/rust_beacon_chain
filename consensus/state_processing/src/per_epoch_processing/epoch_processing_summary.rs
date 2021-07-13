use super::{
    altair::ParticipationCache,
    base::{TotalBalances, ValidatorStatus},
    validator_statuses::InclusionInfo,
};
use safe_arith::ArithError;

/// Provides a summary of validator participation during the epoch.
#[derive(PartialEq, Debug)]
pub enum EpochProcessingSummary {
    Base {
        total_balances: TotalBalances,
        statuses: Vec<ValidatorStatus>,
    },
    Altair {
        participation_cache: ParticipationCache,
    },
}

impl EpochProcessingSummary {
    /// Returns the sum of the effective balance of all validators in the current epoch.
    pub fn current_epoch_total_active_balance(&self) -> u64 {
        match self {
            EpochProcessingSummary::Base { total_balances, .. } => total_balances.current_epoch(),
            EpochProcessingSummary::Altair {
                participation_cache,
            } => participation_cache.current_epoch_total_active_balance(),
        }
    }

    /// Returns the sum of the effective balance of all validators in the current epoch who
    /// included an attestation that matched the target.
    pub fn current_epoch_target_attesting_balance(&self) -> Result<u64, ArithError> {
        match self {
            EpochProcessingSummary::Base { total_balances, .. } => {
                Ok(total_balances.current_epoch_target_attesters())
            }
            EpochProcessingSummary::Altair {
                participation_cache,
            } => participation_cache.current_epoch_target_attesting_balance(),
        }
    }

    /// Returns the sum of the effective balance of all validators in the previous epoch.
    pub fn previous_epoch_total_active_balance(&self) -> u64 {
        match self {
            EpochProcessingSummary::Base { total_balances, .. } => total_balances.previous_epoch(),
            EpochProcessingSummary::Altair {
                participation_cache,
            } => participation_cache.previous_epoch_total_active_balance(),
        }
    }

    /// Returns `true` if `val_index` was included in the active validator indices in the current
    /// epoch.
    ///
    /// ## Notes
    ///
    /// Always returns `false` for an unknown `val_index`.
    pub fn is_active_in_current_epoch(&self, val_index: usize) -> bool {
        match self {
            EpochProcessingSummary::Base { statuses, .. } => statuses
                .get(val_index)
                .map_or(false, |s| s.is_current_epoch_target_attester),
            EpochProcessingSummary::Altair {
                participation_cache,
                ..
            } => participation_cache.is_active_in_current_epoch(val_index),
        }
    }

    /// Returns `true` if `val_index` had a target-matching attestation included on chain in the
    /// current epoch.
    ///
    /// ## Notes
    ///
    /// Always returns `false` for an unknown `val_index`.
    pub fn is_current_epoch_target_attester(&self, val_index: usize) -> bool {
        match self {
            EpochProcessingSummary::Base { statuses, .. } => statuses
                .get(val_index)
                .map_or(false, |s| s.is_current_epoch_target_attester),
            EpochProcessingSummary::Altair {
                participation_cache,
                ..
            } => participation_cache.is_current_epoch_timely_target_attester(val_index),
        }
    }

    /// Returns the sum of the effective balance of all validators in the previous epoch who
    /// included an attestation that matched the target.
    pub fn previous_epoch_target_attesting_balance(&self) -> Result<u64, ArithError> {
        match self {
            EpochProcessingSummary::Base { total_balances, .. } => {
                Ok(total_balances.previous_epoch_target_attesters())
            }
            EpochProcessingSummary::Altair {
                participation_cache,
            } => participation_cache.previous_epoch_target_attesting_balance(),
        }
    }

    /// Returns the sum of the effective balance of all validators in the previous epoch who
    /// included an attestation that matched the head.
    ///
    /// ## Differences between Base and Altair
    ///
    /// - Base: any attestation can match the head.
    /// - Altair: only "timely" attestations can match the head.
    pub fn previous_epoch_head_attesting_balance(&self) -> Result<u64, ArithError> {
        match self {
            EpochProcessingSummary::Base { total_balances, .. } => {
                Ok(total_balances.previous_epoch_head_attesters())
            }
            EpochProcessingSummary::Altair {
                participation_cache,
            } => participation_cache.previous_epoch_head_attesting_balance(),
        }
    }

    /// Returns `true` if `val_index` was included in the active validator indices in the previous
    /// epoch.
    ///
    /// ## Notes
    ///
    /// Always returns `false` for an unknown `val_index`.
    pub fn is_active_in_previous_epoch(&self, val_index: usize) -> bool {
        match self {
            EpochProcessingSummary::Base { statuses, .. } => statuses
                .get(val_index)
                .map_or(false, |s| s.is_active_in_previous_epoch),
            EpochProcessingSummary::Altair {
                participation_cache,
                ..
            } => participation_cache.is_active_in_previous_epoch(val_index),
        }
    }

    /// Returns `true` if `val_index` had a target-matching attestation included on chain in the
    /// previous epoch.
    ///
    /// ## Notes
    ///
    /// Always returns `false` for an unknown `val_index`.
    pub fn is_previous_epoch_target_attester(&self, val_index: usize) -> bool {
        match self {
            EpochProcessingSummary::Base { statuses, .. } => statuses
                .get(val_index)
                .map_or(false, |s| s.is_previous_epoch_target_attester),
            EpochProcessingSummary::Altair {
                participation_cache,
                ..
            } => participation_cache.is_previous_epoch_timely_target_attester(val_index),
        }
    }

    /// Returns `true` if `val_index` had a head-matching attestation included on chain in the
    /// previous epoch.
    ///
    /// ## Differences between Base and Altair
    ///
    /// - Base: any attestation can match the head.
    /// - Altair: only "timely" attestations can match the head.
    ///
    /// ## Notes
    ///
    /// Always returns `false` for an unknown `val_index`.
    pub fn is_previous_epoch_head_attester(&self, val_index: usize) -> bool {
        match self {
            EpochProcessingSummary::Base { statuses, .. } => statuses
                .get(val_index)
                .map_or(false, |s| s.is_previous_epoch_head_attester),
            EpochProcessingSummary::Altair {
                participation_cache,
                ..
            } => participation_cache.is_previous_epoch_timely_head_attester(val_index),
        }
    }

    /// Returns `true` if `val_index` had a source-matching attestation included on chain in the
    /// previous epoch.
    ///
    /// ## Differences between Base and Altair
    ///
    /// - Base: any attestation can match the head.
    /// - Altair: only "timely" attestations can match the source.
    ///
    /// ## Notes
    ///
    /// Always returns `false` for an unknown `val_index`.
    pub fn is_previous_epoch_source_attester(&self, val_index: usize) -> bool {
        match self {
            EpochProcessingSummary::Base { statuses, .. } => statuses
                .get(val_index)
                .map_or(false, |s| s.is_previous_epoch_attester),
            EpochProcessingSummary::Altair {
                participation_cache,
                ..
            } => participation_cache.is_previous_epoch_timely_source_attester(val_index),
        }
    }

    /// Returns information about the inclusion distance for `val_index` for the previous epoch.
    ///
    /// ## Differences between Base and Altair
    ///
    /// - Base: always returns `Some` if the validator had an attestation included on-chain.
    /// - Altair: always returns `None`.
    ///
    /// ## Notes
    ///
    /// Always returns `false` for an unknown `val_index`.
    pub fn previous_epoch_inclusion_info(&self, val_index: usize) -> Option<InclusionInfo> {
        match self {
            EpochProcessingSummary::Base { statuses, .. } => {
                statuses.get(val_index).and_then(|s| s.inclusion_info)
            }
            EpochProcessingSummary::Altair { .. } => None,
        }
    }
}