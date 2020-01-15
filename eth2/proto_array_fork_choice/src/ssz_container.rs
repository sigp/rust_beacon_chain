use crate::{
    proto_array::{ProtoArray, ProtoNode},
    ElasticList, ProtoArrayForkChoice, VoteTracker,
};
use parking_lot::RwLock;
use ssz_derive::{Decode, Encode};
use std::collections::HashMap;
use std::iter::FromIterator;
use types::{Epoch, Hash256};

#[derive(Encode, Decode)]
pub struct SszContainer {
    votes: Vec<VoteTracker>,
    balances: Vec<u64>,
    prune_threshold: usize,
    ffg_update_required: bool,
    justified_epoch: Epoch,
    finalized_epoch: Epoch,
    nodes: Vec<ProtoNode>,
    indices: Vec<(Hash256, usize)>,
}

impl From<&ProtoArrayForkChoice> for SszContainer {
    fn from(from: &ProtoArrayForkChoice) -> Self {
        let proto_array = from.proto_array.read();

        Self {
            votes: from.votes.read().0.clone(),
            balances: from.balances.read().clone(),
            prune_threshold: proto_array.prune_threshold,
            ffg_update_required: proto_array.ffg_update_required,
            justified_epoch: proto_array.justified_epoch,
            finalized_epoch: proto_array.finalized_epoch,
            nodes: proto_array.nodes.clone(),
            indices: proto_array.indices.iter().map(|(k, v)| (*k, *v)).collect(),
        }
    }
}

impl From<SszContainer> for ProtoArrayForkChoice {
    fn from(from: SszContainer) -> Self {
        let proto_array = ProtoArray {
            prune_threshold: from.prune_threshold,
            ffg_update_required: from.ffg_update_required,
            justified_epoch: from.justified_epoch,
            finalized_epoch: from.finalized_epoch,
            nodes: from.nodes,
            indices: HashMap::from_iter(from.indices.into_iter()),
        };

        Self {
            proto_array: RwLock::new(proto_array),
            votes: RwLock::new(ElasticList(from.votes)),
            balances: RwLock::new(from.balances),
        }
    }
}
