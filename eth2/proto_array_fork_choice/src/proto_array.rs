use crate::error::Error;
use serde_derive::{Deserialize, Serialize};
use ssz_derive::{Decode, Encode};
use std::collections::HashMap;
use types::{Epoch, Hash256, Slot};

#[derive(Clone, PartialEq, Debug, Encode, Decode, Serialize, Deserialize)]
pub struct ProtoNode {
    /// The `slot` is not necessary for `ProtoArray`, it just exists so external components can
    /// easily query the block slot. This is useful for upstream fork choice logic.
    pub slot: Slot,
    root: Hash256,
    parent: Option<usize>,
    justified_epoch: Epoch,
    finalized_epoch: Epoch,
    weight: u64,
    best_child: Option<usize>,
    best_descendant: Option<usize>,
}

#[derive(PartialEq, Serialize, Deserialize)]
pub struct ProtoArray {
    /// Do not attempt to prune the tree unless it has at least this many nodes. Small prunes
    /// simply waste time.
    pub prune_threshold: usize,
    pub justified_epoch: Epoch,
    pub finalized_epoch: Epoch,
    pub nodes: Vec<ProtoNode>,
    pub indices: HashMap<Hash256, usize>,
}

impl ProtoArray {
    /// Iterate backwards through the array, touching all nodes and their parents and potentially
    /// the best-child of each parent.
    ///
    /// The structure of the `self.nodes` array ensures that the child of each node is always
    /// touched before it's parent.
    ///
    /// For each node, the following is done:
    ///
    /// - Update the nodes weight with the corresponding delta.
    /// - Back-propgrate each nodes delta to its parents delta.
    /// - Compare the current node with the parents best-child, updating it if the current node
    /// should become the best child.
    /// - Update the parents best-descendant with the current node or its best-descendant, if
    /// required.
    pub fn apply_score_changes(
        &mut self,
        mut deltas: Vec<i64>,
        justified_epoch: Epoch,
        finalized_epoch: Epoch,
    ) -> Result<(), Error> {
        if deltas.len() != self.indices.len() {
            return Err(Error::InvalidDeltaLen {
                deltas: deltas.len(),
                indices: self.indices.len(),
            });
        }

        if justified_epoch != self.justified_epoch || finalized_epoch != self.finalized_epoch {
            self.justified_epoch = justified_epoch;
            self.finalized_epoch = finalized_epoch;
        }

        // Iterate backwards through all indices in `self.nodes`.
        for node_index in (0..self.nodes.len()).rev() {
            let node = &mut self
                .nodes
                .get_mut(node_index)
                .ok_or_else(|| Error::InvalidNodeIndex(node_index))?;

            // There is no need to adjust the balances or manage parent of the zero hash since it
            // is an alias to the genesis block. The weight applied to the genesis block is
            // irrelevant as we _always_ choose it and it's impossible for it to have a parent.
            if node.root == Hash256::zero() {
                continue;
            }

            let node_delta = deltas
                .get(node_index)
                .copied()
                .ok_or_else(|| Error::InvalidNodeDelta(node_index))?;

            // Apply the delta to the node.
            if node_delta < 0 {
                // Note: I am conflicted about whether to use `saturating_sub` or `checked_sub`
                // here.
                //
                // I can't think of any valid reason why `node_delta.abs()` should be greater than
                // `node.weight`, so I have chosen `checked_sub` to try and fail-fast if there is
                // some error.
                //
                // However, I am not fully convinced that some valid case for `saturating_sub` does
                // not exist.
                node.weight = node
                    .weight
                    .checked_sub(node_delta.abs() as u64)
                    .ok_or_else(|| Error::DeltaOverflow(node_index))?;
            } else {
                node.weight = node
                    .weight
                    .checked_add(node_delta as u64)
                    .ok_or_else(|| Error::DeltaOverflow(node_index))?;
            }

            // If the node has a parent, try to update its best-child and best-descendant.
            if let Some(parent_index) = node.parent {
                let parent_delta = deltas
                    .get_mut(parent_index)
                    .ok_or_else(|| Error::InvalidParentDelta(parent_index))?;

                // Back-propogate the nodes delta to its parent.
                *parent_delta += node_delta;

                self.maybe_update_best_child_and_descendant(parent_index, node_index)?;
            }
        }

        Ok(())
    }

    /// Register a new block with the fork choice.
    ///
    /// It is only sane to supply a `None` parent for the genesis block.
    pub fn on_new_block(
        &mut self,
        slot: Slot,
        root: Hash256,
        parent_opt: Option<Hash256>,
        justified_epoch: Epoch,
        finalized_epoch: Epoch,
    ) -> Result<(), Error> {
        let node_index = self.nodes.len();

        let node = ProtoNode {
            slot,
            root,
            parent: parent_opt.and_then(|parent| self.indices.get(&parent).copied()),
            justified_epoch,
            finalized_epoch,
            weight: 0,
            best_child: None,
            best_descendant: None,
        };

        self.indices.insert(node.root, node_index);
        self.nodes.push(node.clone());

        if let Some(parent_index) = node.parent {
            self.maybe_update_best_child_and_descendant(parent_index, node_index)?;
        }

        Ok(())
    }

    /// Follows the best-descendant links to find the best-block (i.e., head-block).
    ///
    /// ## Notes
    ///
    /// The result of this function is not guaranteed to be accurate if `Self::on_new_block` has
    /// been called without a subsequent `Self::apply_score_changes` call. This is because
    /// `on_new_block` does not attempt to walk backwards through the tree and update the
    /// best-child/best-descendant links.
    pub fn find_head(&self, justified_root: &Hash256) -> Result<Hash256, Error> {
        let justified_index = self
            .indices
            .get(justified_root)
            .copied()
            .ok_or_else(|| Error::JustifiedNodeUnknown(*justified_root))?;

        let justified_node = self
            .nodes
            .get(justified_index)
            .ok_or_else(|| Error::InvalidJustifiedIndex(justified_index))?;

        let best_descendant_index = justified_node
            .best_descendant
            .unwrap_or_else(|| justified_index);

        let best_node = self
            .nodes
            .get(best_descendant_index)
            .ok_or_else(|| Error::InvalidBestDescendant(best_descendant_index))?;

        // It is a logic error to try and find the head starting from a block that does not match
        // the filter.
        if !self.node_is_viable_for_head(&best_node) {
            return Err(Error::InvalidBestNode {
                justified_epoch: self.justified_epoch,
                finalized_epoch: self.finalized_epoch,
                node_justified_epoch: justified_node.justified_epoch,
                node_finalized_epoch: justified_node.finalized_epoch,
            });
        }

        // dbg!(&self.nodes);

        Ok(best_node.root)
    }

    /// Update the tree with new finalization information. The tree is only actually pruned if both
    /// of the two following criteria are met:
    ///
    /// - The supplied finalized epoch and root are different to the current values.
    /// - The number of nodes in `self` is at least `self.prune_threshold`.
    ///
    /// # Errors
    ///
    /// Returns errors if:
    ///
    /// - The finalized epoch is less than the current one.
    /// - The finalized epoch is equal to the current one, but the finalized root is different.
    /// - There is some internal error relating to invalid indices inside `self`.
    pub fn maybe_prune(
        &mut self,
        finalized_epoch: Epoch,
        finalized_root: Hash256,
    ) -> Result<(), Error> {
        if finalized_epoch < self.finalized_epoch {
            // It's illegal to swap to an earlier finalized root (this is assumed to be reverting a
            // finalized block).
            return Err(Error::RevertedFinalizedEpoch {
                current_finalized_epoch: self.finalized_epoch,
                new_finalized_epoch: finalized_epoch,
            });
        } else if finalized_epoch != self.finalized_epoch {
            self.finalized_epoch = finalized_epoch;
        }

        let finalized_index = *self
            .indices
            .get(&finalized_root)
            .ok_or_else(|| Error::FinalizedNodeUnknown(finalized_root))?;

        if finalized_index < self.prune_threshold {
            // Pruning at small numbers incurs more cost than benefit.
            return Ok(());
        }

        // Remove the `self.indices` key/values for all the to-be-deleted nodes.
        for node_index in 0..finalized_index {
            let root = &self
                .nodes
                .get(node_index)
                .ok_or_else(|| Error::InvalidNodeIndex(node_index))?
                .root;
            self.indices.remove(root);
        }

        // Drop all the nodes prior to finalization.
        self.nodes = self.nodes.split_off(finalized_index);

        // Adjust the indices map.
        for (_root, index) in self.indices.iter_mut() {
            *index = index
                .checked_sub(finalized_index)
                .ok_or_else(|| Error::IndexOverflow("indices"))?;
        }

        // Iterate through all the existing nodes and adjust their indices to match the new layout
        // of `self.nodes`.
        for node in self.nodes.iter_mut() {
            if let Some(parent) = node.parent {
                // If `node.parent` is less than `finalized_index`, set it to `None`.
                node.parent = parent.checked_sub(finalized_index);
            }
            if let Some(best_child) = node.best_child {
                node.best_child = Some(
                    best_child
                        .checked_sub(finalized_index)
                        .ok_or_else(|| Error::IndexOverflow("best_child"))?,
                );
            }
            if let Some(best_descendant) = node.best_descendant {
                node.best_descendant = Some(
                    best_descendant
                        .checked_sub(finalized_index)
                        .ok_or_else(|| Error::IndexOverflow("best_descendant"))?,
                );
            }
        }

        Ok(())
    }

    /// Observe the parent at `parent_index` with respect to the child at `child_index` and
    /// potentially modify the `parent.best_child` and `parent.best_descendant` values.
    ///
    /// ## Detail
    ///
    /// There are four outcomes:
    ///
    /// - The child is already the best child but it's now invalid due to a FFG change and should be removed.
    /// - The child is already the best child and the parent is updated with the new
    ///     best-descendant.
    /// - The child is not the best child but becomes the best child.
    /// - The child is not the best child and does not become the best child.
    fn maybe_update_best_child_and_descendant(
        &mut self,
        parent_index: usize,
        child_index: usize,
    ) -> Result<(), Error> {
        let child = self
            .nodes
            .get(child_index)
            .ok_or_else(|| Error::InvalidNodeIndex(child_index))?;

        let parent = self
            .nodes
            .get(parent_index)
            .ok_or_else(|| Error::InvalidNodeIndex(parent_index))?;

        let child_leads_to_viable_head = self.node_leads_to_viable_head(&child)?;

        // These three variables are aliases to the three options that we may set the
        // `parent.best_child` and `parent.best_descendant` to.
        //
        // I use the aliases to assist readability.
        let change_to_none = (None, None);
        let change_to_child = (
            Some(child_index),
            child.best_descendant.or(Some(child_index)),
        );
        let no_change = (parent.best_child, parent.best_descendant);

        let (new_best_child, new_best_descendant) =
            if let Some(best_child_index) = parent.best_child {
                if best_child_index == child_index && !child_leads_to_viable_head {
                    // If the child is already the best-child of the parent but it's not viable for
                    // the head, remove it.
                    change_to_none
                } else if best_child_index == child_index {
                    // If the child is the best-child already, set it again to ensure that the
                    // best-descendant of the parent is updated.
                    change_to_child
                } else {
                    let best_child = self
                        .nodes
                        .get(best_child_index)
                        .ok_or_else(|| Error::InvalidBestDescendant(best_child_index))?;

                    let child_leads_to_viable_head = self.node_leads_to_viable_head(&child)?;
                    let best_child_leads_to_viable_head =
                        self.node_leads_to_viable_head(&best_child)?;

                    if child_leads_to_viable_head && !best_child_leads_to_viable_head {
                        // The child leads to a viable head, but the current best-child doesn't.
                        change_to_child
                    } else if !child_leads_to_viable_head && best_child_leads_to_viable_head {
                        // The best child leads to a viable head, but the child doesn't.
                        no_change
                    } else if child.weight == best_child.weight {
                        // Tie-breaker of equal weights by root.
                        if child.root >= best_child.root {
                            change_to_child
                        } else {
                            no_change
                        }
                    } else {
                        // Choose the winner by weight.
                        if child.weight >= best_child.weight {
                            change_to_child
                        } else {
                            no_change
                        }
                    }
                }
            } else {
                if child_leads_to_viable_head {
                    // There is no current best-child and the child is viable.
                    change_to_child
                } else {
                    // There is no current best-child but the child is not viable.
                    no_change
                }
            };

        let parent = self
            .nodes
            .get_mut(parent_index)
            .ok_or_else(|| Error::InvalidNodeIndex(parent_index))?;

        parent.best_child = new_best_child;
        parent.best_descendant = new_best_descendant;

        Ok(())
    }

    /// Indicates if the node itself is viable for the head, or if it's best descendant is viable
    /// for the head.
    fn node_leads_to_viable_head(&self, node: &ProtoNode) -> Result<bool, Error> {
        let best_descendant_is_viable_for_head =
            if let Some(best_descendant_index) = node.best_descendant {
                let best_descendant = self
                    .nodes
                    .get(best_descendant_index)
                    .ok_or_else(|| Error::InvalidBestDescendant(best_descendant_index))?;

                self.node_is_viable_for_head(best_descendant)
            } else {
                false
            };

        Ok(best_descendant_is_viable_for_head || self.node_is_viable_for_head(node))
    }

    /// This is the equivalent to the `filter_block_tree` function in the eth2 spec:
    ///
    /// https://github.com/ethereum/eth2.0-specs/blob/v0.10.0/specs/phase0/fork-choice.md#filter_block_tree
    ///
    /// Any node that has a different finalized or justified epoch should not be viable for the
    /// head.
    fn node_is_viable_for_head(&self, node: &ProtoNode) -> bool {
        (node.justified_epoch == self.justified_epoch || self.justified_epoch == Epoch::new(0))
            && (node.finalized_epoch == self.finalized_epoch
                || self.finalized_epoch == Epoch::new(0))
    }
}
