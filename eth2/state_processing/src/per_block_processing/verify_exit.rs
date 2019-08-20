use super::errors::{BlockOperationError, ExitInvalid};
use tree_hash::SignedRoot;
use types::*;

type Result<T> = std::result::Result<T, BlockOperationError<ExitInvalid>>;

fn error(reason: ExitInvalid) -> BlockOperationError<ExitInvalid> {
    BlockOperationError::invalid(reason)
}

/// Indicates if an `Exit` is valid to be included in a block in the current epoch of the given
/// state.
///
/// Returns `Ok(())` if the `Exit` is valid, otherwise indicates the reason for invalidity.
///
/// Spec v0.8.0
pub fn verify_exit<T: EthSpec>(
    state: &BeaconState<T>,
    exit: &VoluntaryExit,
    spec: &ChainSpec,
) -> Result<()> {
    verify_exit_parametric(state, exit, spec, false)
}

/// Like `verify_exit` but doesn't run checks which may become true in future states.
///
/// Spec v0.8.0
pub fn verify_exit_time_independent_only<T: EthSpec>(
    state: &BeaconState<T>,
    exit: &VoluntaryExit,
    spec: &ChainSpec,
) -> Result<()> {
    verify_exit_parametric(state, exit, spec, true)
}

/// Parametric version of `verify_exit` that skips some checks if `time_independent_only` is true.
///
/// Spec v0.8.0
fn verify_exit_parametric<T: EthSpec>(
    state: &BeaconState<T>,
    exit: &VoluntaryExit,
    spec: &ChainSpec,
    time_independent_only: bool,
) -> Result<()> {
    let validator = state
        .validators
        .get(exit.validator_index as usize)
        .ok_or_else(|| error(ExitInvalid::ValidatorUnknown(exit.validator_index)))?;

    // Verify the validator is active.
    verify!(
        validator.is_active_at(state.current_epoch()),
        ExitInvalid::NotActive(exit.validator_index)
    );

    // Verify that the validator has not yet exited.
    verify!(
        validator.exit_epoch == spec.far_future_epoch,
        ExitInvalid::AlreadyExited(exit.validator_index)
    );

    // Exits must specify an epoch when they become valid; they are not valid before then.
    verify!(
        time_independent_only || state.current_epoch() >= exit.epoch,
        ExitInvalid::FutureEpoch {
            state: state.current_epoch(),
            exit: exit.epoch
        }
    );

    // Verify the validator has been active long enough.
    verify!(
        state.current_epoch() >= validator.activation_epoch + spec.persistent_committee_period,
        ExitInvalid::TooYoungToExit {
            current_epoch: state.current_epoch(),
            earliest_exit_epoch: validator.activation_epoch + spec.persistent_committee_period,
        }
    );

    // Verify signature.
    let message = exit.signed_root();
    let domain = spec.get_domain(exit.epoch, Domain::VoluntaryExit, &state.fork);
    verify!(
        exit.signature
            .verify(&message[..], domain, &validator.pubkey),
        ExitInvalid::BadSignature
    );

    Ok(())
}
