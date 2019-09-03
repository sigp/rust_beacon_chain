use crate::BeaconChainTypes;
use eth2_hashing::hash;
use std::marker::PhantomData;
use types::{BeaconState, Deposit, DepositData, Eth1Data, EthSpec, Hash256};

type Result<T> = std::result::Result<T, Error>;

pub enum Error {
    /// Unable to return an Eth1Data for the given epoch.
    EpochUnavailable,
    /// An error from the backend service (e.g., the web3 data fetcher).
    BackendError(String),
}

pub trait Eth1Chain<T: BeaconChainTypes> {
    /// Returns the `Eth1Data` that should be included in a block being produced for the given
    /// `state`.
    fn eth1_data_for_epoch(&self, beacon_state: &BeaconState<T::EthSpec>) -> Result<Eth1Data>;

    /// Returns all `Deposits` between `state.eth1_deposit_index` and
    /// `state.eth1_data.deposit_count`.
    ///
    /// # Note:
    ///
    /// It is possible that not all returned `Deposits` can be included in a block. E.g., there may
    /// be more than `MAX_DEPOSIT_COUNT` or the churn may be too high.
    fn queued_deposits(&self, beacon_state: &BeaconState<T::EthSpec>) -> Result<Vec<Deposit>>;
}

pub struct InteropEth1Chain<T: BeaconChainTypes> {
    _phantom: PhantomData<T>,
}

impl<T: BeaconChainTypes> Eth1Chain<T> for InteropEth1Chain<T> {
    fn eth1_data_for_epoch(&self, state: &BeaconState<T::EthSpec>) -> Result<Eth1Data> {
        let current_epoch = state.current_epoch();
        let slots_per_voting_period = T::EthSpec::slots_per_eth1_voting_period() as u64;
        let current_voting_period: u64 = current_epoch.as_u64() / slots_per_voting_period;

        // TODO: confirm that `int_to_bytes32` is correct.
        let deposit_root = hash(&int_to_bytes32(current_voting_period));
        let block_hash = hash(&deposit_root);

        Ok(Eth1Data {
            deposit_root: Hash256::from_slice(&deposit_root),
            deposit_count: state.eth1_deposit_index,
            block_hash: Hash256::from_slice(&block_hash),
        })
    }

    fn queued_deposits(&self, beacon_state: &BeaconState<T::EthSpec>) -> Result<Vec<Deposit>> {
        Ok(vec![])
    }
}

/// Returns `int` as little-endian bytes with a length of 32.
fn int_to_bytes32(int: u64) -> Vec<u8> {
    let mut vec = int.to_le_bytes().to_vec();
    vec.resize(32, 0);
    vec
}
