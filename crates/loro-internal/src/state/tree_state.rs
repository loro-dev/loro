use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::{
    ContainerID, IdFull, LoroError, LoroResult, LoroTreeError, LoroValue, TreeID, ID,
};
use rle::HasLength;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Weak};

use crate::container::idx::ContainerIdx;
use crate::delta::{TreeDiff, TreeDiffItem, TreeExternalDiff};
use crate::encoding::{EncodeMode, StateSnapshotDecodeContext, StateSnapshotEncoder};
use crate::event::InternalDiff;
use crate::txn::Transaction;
use crate::DocState;
use crate::{
    arena::SharedArena,
    container::tree::tree_op::TreeOp,
    delta::TreeInternalDiff,
    event::{Diff, Index},
    op::RawOp,
};

use super::ContainerState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumAsInner, Serialize)]
pub enum TreeParentId {
    Node(TreeID),
    Unexist,
    Deleted,
    /// parent is root
    None,
}

/// The state of movable tree.
///
/// using flat representation
#[derive(Debug, Clone)]
pub struct TreeState {
    idx: ContainerIdx,
    pub(crate) trees: FxHashMap<TreeID, TreeStateNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TreeStateNode {
    pub parent: TreeParentId,
    pub last_move_op: IdFull,
}

impl TreeState {
    pub fn new(idx: ContainerIdx) -> Self {
        Self {
            idx,
            trees: FxHashMap::default(),
        }
    }

    pub fn mov(
        &mut self,
        target: TreeID,
        parent: TreeParentId,
        id: IdFull,
    ) -> Result<(), LoroError> {
        if parent.is_none() {
            // new root node
            self.trees.insert(
                target,
                TreeStateNode {
                    parent,
                    last_move_op: id,
                },
            );
            return Ok(());
        };
        if let TreeParentId::Node(parent) = parent {
            if !self.trees.contains_key(&parent) {
                return Err(LoroTreeError::TreeNodeParentNotFound(parent).into());
            }
        }
        if self.is_ancestor_of(&target, &parent) {
            return Err(LoroTreeError::CyclicMoveError.into());
        }
        // move or delete or create children node
        self.trees.insert(
            target,
            TreeStateNode {
                parent,
                last_move_op: id,
            },
        );
        Ok(())
    }

    #[inline(never)]
    fn is_ancestor_of(&self, maybe_ancestor: &TreeID, node_id: &TreeParentId) -> bool {
        if !self.trees.contains_key(maybe_ancestor) {
            return false;
        }
        if let TreeParentId::Node(id) = node_id {
            if id == maybe_ancestor {
                return true;
            }
        }
        match node_id {
            TreeParentId::Node(id) => {
                let parent = &self.trees.get(id).unwrap().parent;
                if parent == node_id {
                    panic!("is_ancestor_of loop")
                }
                self.is_ancestor_of(maybe_ancestor, parent)
            }
            TreeParentId::Deleted | TreeParentId::None => false,
            TreeParentId::Unexist => unreachable!(),
        }
    }

    pub fn contains(&self, target: TreeID) -> bool {
        !self.is_node_deleted(&target)
    }

    /// Get the parent of the node, if the node is deleted or does not exist, return None
    pub fn parent(&self, target: TreeID) -> Option<TreeParentId> {
        if self.is_node_deleted(&target) {
            None
        } else {
            self.trees.get(&target).map(|x| x.parent)
        }
    }

    /// If the node is not deleted or does not exist, return false.
    /// only the node is deleted and exists, return true
    fn is_node_deleted(&self, target: &TreeID) -> bool {
        match self.trees.get(target) {
            Some(x) => match x.parent {
                TreeParentId::Deleted => true,
                TreeParentId::None => false,
                TreeParentId::Node(p) => self.is_node_deleted(&p),
                TreeParentId::Unexist => unreachable!(),
            },
            None => false,
        }
    }

    pub fn nodes(&self) -> Vec<TreeID> {
        self.trees
            .keys()
            .filter(|&k| !self.is_node_deleted(k))
            .copied()
            .collect::<Vec<_>>()
    }

    #[cfg(feature = "test_utils")]
    pub fn max_counter(&self) -> i32 {
        self.trees
            .keys()
            .filter(|&k| !self.is_node_deleted(k))
            .map(|k| k.counter)
            .max()
            .unwrap_or(0)
    }

    pub fn get_children(&self, parent: &TreeParentId) -> Vec<TreeID> {
        let mut ans = Vec::new();
        for (t, p) in self.trees.iter() {
            if &p.parent == parent {
                ans.push(*t);
            }
        }
        ans
    }
}

impl ContainerState for TreeState {
    fn container_idx(&self) -> crate::container::idx::ContainerIdx {
        self.idx
    }

    fn estimate_size(&self) -> usize {
        self.trees.len() * (std::mem::size_of::<(TreeID, TreeStateNode)>())
    }

    fn is_state_empty(&self) -> bool {
        self.nodes().is_empty()
    }

    fn apply_diff_and_convert(
        &mut self,
        diff: crate::event::InternalDiff,
        _arena: &SharedArena,
        _txn: &Weak<Mutex<Option<Transaction>>>,
        _state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        if let InternalDiff::Tree(tree) = &diff {
            // assert never cause cycle move
            for diff in tree.diff.iter() {
                let target = diff.target;
                // create associated metadata container
                let parent = match diff.action {
                    TreeInternalDiff::Create(p)
                    | TreeInternalDiff::Move(p)
                    | TreeInternalDiff::Delete(p)
                    | TreeInternalDiff::MoveInDelete(p) => p,
                    TreeInternalDiff::UnCreate => {
                        // delete it from state
                        self.trees.remove(&target);
                        continue;
                    }
                };
                self.trees.insert(
                    target,
                    TreeStateNode {
                        parent,
                        last_move_op: diff.last_effective_move_op_id,
                    },
                );
            }
        }
        let ans = diff
            .into_tree()
            .unwrap()
            .diff
            .into_iter()
            .filter_map(TreeDiffItem::from_delta_item)
            .collect_vec();
        Diff::Tree(TreeDiff { diff: ans })
    }

    fn apply_diff(
        &mut self,
        diff: InternalDiff,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) {
        let _ = self.apply_diff_and_convert(diff, arena, txn, state);
    }

    fn apply_local_op(&mut self, raw_op: &RawOp, _op: &Op) -> LoroResult<()> {
        match &raw_op.content {
            crate::op::RawOpContent::Tree(tree) => match tree {
                TreeOp::Create {
                    target,
                    parent,
                }
                | TreeOp::Move {
                    target,
                    parent,
                } => {
                    let parent = TreeParentId::from(*parent);
                    self.mov(
                        *target,
                        parent,
                        raw_op.id_full(),
                    )
                }
                TreeOp::Delete { target } => {
                    let parent = TreeParentId::Deleted;
                    self.mov(*target, parent, raw_op.id_full())
                }
            },
            _ => unreachable!(),
        }
    }

    fn to_diff(
        &mut self,
        _arena: &SharedArena,
        _txn: &Weak<Mutex<Option<Transaction>>>,
        _state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        let mut diffs = vec![];
        // TODO: perf
        let forest = Forest::from_tree_state(&self.trees);
        let mut q = VecDeque::from(forest.roots);
        while let Some(node) = q.pop_front() {
            let parent = if let Some(p) = node.parent {
                TreeParentId::Node(p)
            } else {
                TreeParentId::None
            };
            let diff = TreeDiffItem {
                target: node.id,
                action: TreeExternalDiff::Create(parent.into_node().ok()),
            };
            diffs.push(diff);
            q.extend(node.children);
        }

        Diff::Tree(TreeDiff { diff: diffs })
    }

    fn get_value(&mut self) -> LoroValue {
        let mut ans: Vec<LoroValue> = vec![];
        #[cfg(feature = "test_utils")]
        // The order keep consistent
        let iter = self.trees.keys().sorted();
        #[cfg(not(feature = "test_utils"))]
        let iter = self.trees.keys();
        for target in iter {
            if !self.is_node_deleted(target) {
                let node = self.trees.get(target).unwrap();
                let mut t = FxHashMap::default();
                t.insert("id".to_string(), target.id().to_string().into());
                let p = node
                    .parent
                    .as_node()
                    .map(|p| p.to_string().into())
                    .unwrap_or(LoroValue::Null);
                t.insert("parent".to_string(), p);
                t.insert(
                    "meta".to_string(),
                    target.associated_meta_container().into(),
                );
                ans.push(t.into());
            }
        }
        ans.sort_by_key(|x| {
            let id: ID = x
                .as_map()
                .unwrap()
                .get("id")
                .unwrap()
                .as_string()
                .unwrap()
                .as_str()
                .try_into()
                .unwrap();
            id
        });
        ans.into()
    }

    /// Get the index of the child container
    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        let id = id.as_normal().unwrap();
        let tree_id = TreeID {
            peer: *id.0,
            counter: *id.1,
        };
        if !self.trees.contains_key(&tree_id) || self.is_node_deleted(&tree_id) {
            None
        } else {
            Some(Index::Node(tree_id))
        }
    }

    fn get_child_containers(&self) -> Vec<ContainerID> {
        self.nodes()
            .into_iter()
            .map(|n| n.associated_meta_container())
            .collect_vec()
    }

    #[doc = " Get a list of ops that can be used to restore the state to the current state"]
    fn encode_snapshot(&self, mut encoder: StateSnapshotEncoder) -> Vec<u8> {
        for node in self.trees.values() {
            if node.last_move_op == IdFull::NONE_ID {
                continue;
            }
            encoder.encode_op(node.last_move_op.idlp().into(), || unimplemented!());
        }

        Vec::new()
    }

    #[doc = " Restore the state to the state represented by the ops that exported by `get_snapshot_ops`"]
    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext) {
        assert_eq!(ctx.mode, EncodeMode::Snapshot);
        for op in ctx.ops {
            assert_eq!(op.op.atom_len(), 1);
            let content = op.op.content.as_tree().unwrap();
            match content {
                TreeOp::Create {
                    target,
                    parent,
                }
                | TreeOp::Move {
                    target,
                    parent,
                } => {
                    let parent = TreeParentId::from(*parent);
                    self.mov(*target, parent, op.id_full())
                        .unwrap()
                }
                TreeOp::Delete { target } => {
                    let parent = TreeParentId::Deleted;
                    self.mov(*target, parent, op.id_full())
                        .unwrap()
                }
            };
        }
    }
}

// convert map container to LoroValue
#[allow(clippy::ptr_arg)]
pub(crate) fn get_meta_value(nodes: &mut Vec<LoroValue>, state: &mut DocState) {
    for node in nodes.iter_mut() {
        let map = Arc::make_mut(node.as_map_mut().unwrap());
        let meta = map.get_mut("meta").unwrap();
        let id = meta.as_container().unwrap();
        *meta = state.get_container_deep_value(state.arena.register_container(id));
    }
}
