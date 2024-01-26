use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::{ContainerID, LoroError, LoroResult, LoroTreeError, LoroValue, TreeID, ID};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumAsInner)]
pub enum TreeParentId {
    Node(TreeID),
    Unexist,
    Deleted,
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
    pub last_move_op: ID,
}

// impl Ord for TreeStateNode {
//     fn cmp(&self, other: &Self) -> std::cmp::Ordering {
//         self.parent.cmp(&other.parent)
//     }
// }

// impl PartialOrd for TreeStateNode {
//     fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
//         Some(self.cmp(other))
//     }
// }

impl TreeState {
    pub fn new(idx: ContainerIdx) -> Self {
        Self {
            idx,
            trees: FxHashMap::default(),
        }
    }

    pub fn mov(&mut self, target: TreeID, parent: TreeParentId, id: ID) -> Result<(), LoroError> {
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
        match node_id {
            TreeParentId::Node(node_id) => {
                if maybe_ancestor == node_id {
                    return true;
                }
                let mut cur_node_id = node_id;
                loop {
                    let parent = &self.trees.get(cur_node_id).unwrap().parent;
                    match parent {
                        TreeParentId::Node(parent_id) => {
                            if parent_id == maybe_ancestor {
                                return true;
                            }
                            if parent_id == cur_node_id {
                                panic!("loop detected")
                            }
                            cur_node_id = parent_id;
                        }
                        TreeParentId::Deleted | TreeParentId::None => return false,
                        TreeParentId::Unexist => unreachable!(),
                    }
                }
            }
            TreeParentId::Deleted | TreeParentId::None => false,
            TreeParentId::Unexist => unreachable!(),
        }
    }

    pub fn contains(&self, target: TreeID) -> bool {
        if TreeID::is_deleted_root(&target) {
            return true;
        }
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

    pub fn get_children_with_id(&self, parent: &TreeParentId) -> Vec<(TreeID, ID)> {
        let mut ans = Vec::new();
        for (t, p) in self.trees.iter() {
            if &p.parent == parent {
                ans.push((*t, p.last_move_op));
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
                    TreeInternalDiff::Create
                    | TreeInternalDiff::Restore
                    | TreeInternalDiff::AsRoot => TreeParentId::None,
                    TreeInternalDiff::Move(parent)
                    | TreeInternalDiff::CreateMove(parent)
                    | TreeInternalDiff::RestoreMove(parent) => TreeParentId::Node(parent),
                    TreeInternalDiff::Delete => TreeParentId::Deleted,
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
            .map(TreeDiffItem::from_delta_item)
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
        self.apply_diff_and_convert(diff, arena, txn, state);
    }

    fn apply_local_op(&mut self, raw_op: &RawOp, _op: &crate::op::Op) -> LoroResult<()> {
        match raw_op.content {
            crate::op::RawOpContent::Tree(tree) => {
                let TreeOp { target, parent, .. } = tree;
                // TODO: use TreeParentId
                let parent = match parent {
                    Some(parent) => {
                        if TreeID::is_deleted_root(&parent) {
                            TreeParentId::Deleted
                        } else {
                            TreeParentId::Node(parent)
                        }
                    }
                    None => TreeParentId::None,
                };
                self.mov(target, parent, raw_op.id)
            }
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
            let diff = TreeDiffItem {
                target: node.id,
                action: TreeExternalDiff::Create(node.parent),
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
        ans.into()
    }

    /// Get the index of the child container
    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        let id = id.as_normal().unwrap();
        Some(Index::Node(TreeID {
            peer: *id.0,
            counter: *id.1,
        }))
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
            if node.last_move_op == ID::NONE_ID {
                continue;
            }
            encoder.encode_op(node.last_move_op.into(), || unimplemented!());
        }

        Vec::new()
    }

    #[doc = " Restore the state to the state represented by the ops that exported by `get_snapshot_ops`"]
    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext) {
        assert_eq!(ctx.mode, EncodeMode::Snapshot);
        for op in ctx.ops {
            assert_eq!(op.op.atom_len(), 1);
            let content = op.op.content.as_tree().unwrap();
            let target = content.target;
            let parent = content.parent;
            // TODO: use TreeParentId
            let parent = match parent {
                Some(parent) => {
                    if TreeID::is_deleted_root(&parent) {
                        TreeParentId::Deleted
                    } else {
                        TreeParentId::Node(parent)
                    }
                }
                None => TreeParentId::None,
            };
            self.trees.insert(
                target,
                TreeStateNode {
                    parent,
                    last_move_op: op.id(),
                },
            );
        }
    }
}

/// Convert flatten tree structure to hierarchy for user interface.
///
/// ```json
/// {
///     "roots": [......],
///     // "deleted": [......]
/// }
/// ```
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Forest {
    pub roots: Vec<TreeNode>,
    // deleted: Vec<TreeNode>,
}

/// The node with metadata in hierarchy tree structure.
#[derive(Debug, Serialize, Deserialize)]
pub struct TreeNode {
    id: TreeID,
    meta: LoroValue,
    parent: Option<TreeID>,
    children: Vec<TreeNode>,
}

impl Forest {
    pub(crate) fn from_tree_state(state: &FxHashMap<TreeID, TreeStateNode>) -> Self {
        let mut forest = Self::default();
        let mut parent_id_to_children = FxHashMap::default();

        for id in state.keys().sorted() {
            let parent = state.get(id).unwrap();
            parent_id_to_children
                .entry(parent.parent)
                .or_insert_with(Vec::new)
                .push(*id)
        }

        if let Some(roots) = parent_id_to_children.get(&TreeParentId::None) {
            for root in roots.iter().copied() {
                let mut stack = vec![(
                    root,
                    TreeNode {
                        id: root,
                        parent: None,
                        meta: LoroValue::Container(root.associated_meta_container()),
                        children: vec![],
                    },
                )];
                let mut id_to_node = FxHashMap::default();
                while let Some((id, mut node)) = stack.pop() {
                    if let Some(children) = parent_id_to_children.get(&TreeParentId::Node(id)) {
                        let mut children_to_stack = Vec::new();
                        for child in children {
                            if let Some(child_node) = id_to_node.remove(child) {
                                node.children.push(child_node);
                            } else {
                                children_to_stack.push((
                                    *child,
                                    TreeNode {
                                        id: *child,
                                        parent: Some(id),
                                        meta: LoroValue::Container(
                                            child.associated_meta_container(),
                                        ),
                                        children: vec![],
                                    },
                                ));
                            }
                        }
                        if !children_to_stack.is_empty() {
                            stack.push((id, node));
                            stack.extend(children_to_stack);
                        } else {
                            id_to_node.insert(id, node);
                        }
                    } else {
                        id_to_node.insert(id, node);
                    }
                }
                let root_node = id_to_node.remove(&root).unwrap();
                forest.roots.push(root_node);
            }
        }
        forest
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

#[cfg(test)]
mod tests {
    use super::*;
    const ID1: TreeID = TreeID {
        peer: 0,
        counter: 0,
    };
    const ID2: TreeID = TreeID {
        peer: 0,
        counter: 1,
    };
    const ID3: TreeID = TreeID {
        peer: 0,
        counter: 2,
    };
    const ID4: TreeID = TreeID {
        peer: 0,
        counter: 3,
    };

    #[test]
    fn test_tree_state() {
        let mut state = TreeState::new(ContainerIdx::from_index_and_type(
            0,
            loro_common::ContainerType::Tree,
        ));
        state.mov(ID1, TreeParentId::None, ID::NONE_ID).unwrap();
        state
            .mov(ID2, TreeParentId::Node(ID1), ID::NONE_ID)
            .unwrap();
    }

    #[test]
    fn tree_convert() {
        let mut state = TreeState::new(ContainerIdx::from_index_and_type(
            0,
            loro_common::ContainerType::Tree,
        ));
        state.mov(ID1, TreeParentId::None, ID::NONE_ID).unwrap();
        state
            .mov(ID2, TreeParentId::Node(ID1), ID::NONE_ID)
            .unwrap();
        let roots = Forest::from_tree_state(&state.trees);
        let json = serde_json::to_string(&roots).unwrap();
        assert_eq!(
            json,
            r#"{"roots":[{"id":{"peer":0,"counter":0},"meta":{"Container":{"Normal":{"peer":0,"counter":0,"container_type":"Map"}}},"parent":null,"children":[{"id":{"peer":0,"counter":1},"meta":{"Container":{"Normal":{"peer":0,"counter":1,"container_type":"Map"}}},"parent":{"peer":0,"counter":0},"children":[]}]}]}"#
        )
    }

    #[test]
    fn delete_node() {
        let mut state = TreeState::new(ContainerIdx::from_index_and_type(
            0,
            loro_common::ContainerType::Tree,
        ));
        state.mov(ID1, TreeParentId::None, ID::NONE_ID).unwrap();
        state
            .mov(ID2, TreeParentId::Node(ID1), ID::NONE_ID)
            .unwrap();
        state
            .mov(ID3, TreeParentId::Node(ID2), ID::NONE_ID)
            .unwrap();
        state
            .mov(ID4, TreeParentId::Node(ID1), ID::NONE_ID)
            .unwrap();
        state.mov(ID2, TreeParentId::Deleted, ID::NONE_ID).unwrap();
        let roots = Forest::from_tree_state(&state.trees);
        let json = serde_json::to_string(&roots).unwrap();
        assert_eq!(
            json,
            r#"{"roots":[{"id":{"peer":0,"counter":0},"meta":{"Container":{"Normal":{"peer":0,"counter":0,"container_type":"Map"}}},"parent":null,"children":[{"id":{"peer":0,"counter":3},"meta":{"Container":{"Normal":{"peer":0,"counter":3,"container_type":"Map"}}},"parent":{"peer":0,"counter":0},"children":[]}]}]}"#
        )
    }
}
