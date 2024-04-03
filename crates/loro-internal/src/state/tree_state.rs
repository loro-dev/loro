use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::{
    ContainerID, IdFull, Lamport, LoroError, LoroResult, LoroTreeError, LoroValue, PeerID, TreeID,
};
use rle::HasLength;
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, Weak};

use crate::container::idx::ContainerIdx;
use crate::container::tree::fractional_index::FracIndex;
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

impl From<Option<TreeID>> for TreeParentId {
    fn from(id: Option<TreeID>) -> Self {
        match id {
            Some(id) => {
                if TreeID::is_deleted_root(&id) {
                    TreeParentId::Deleted
                } else {
                    TreeParentId::Node(id)
                }
            }
            None => TreeParentId::None,
        }
    }
}

/// The state of movable tree.
///
/// using flat representation
#[derive(Debug, Clone)]
pub struct TreeState {
    idx: ContainerIdx,
    trees: FxHashMap<TreeID, TreeStateNode>,
    children: FxHashMap<TreeParentId, BTreeMap<NodePosition, TreeID>>,
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord)]
struct NodePosition {
    position: FracIndex,
    id: (Lamport, PeerID),
}

impl NodePosition {
    fn new(position: FracIndex, id: IdFull) -> Self {
        Self {
            position,
            id: (id.lamport, id.peer),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeStateNode {
    pub parent: TreeParentId,
    // no position in delete?
    pub position: Option<FracIndex>,
    pub last_move_op: IdFull,
}

impl TreeState {
    pub fn new(idx: ContainerIdx) -> Self {
        Self {
            idx,
            trees: FxHashMap::default(),
            children: FxHashMap::default(),
        }
    }

    pub fn mov(
        &mut self,
        target: TreeID,
        parent: TreeParentId,
        id: IdFull,
        position: Option<FracIndex>,
        with_check: bool,
    ) -> Result<(), LoroError> {
        if with_check {
            if let TreeParentId::Node(parent) = parent {
                if !self.trees.contains_key(&parent) {
                    return Err(LoroTreeError::TreeNodeParentNotFound(parent).into());
                }
            }
            if self.is_ancestor_of(&target, &parent) {
                return Err(LoroTreeError::CyclicMoveError.into());
            }
        }
        // move or delete or create children node
        let old_parent = self
            .trees
            .get(&target)
            .map(|x| x.parent)
            .unwrap_or(TreeParentId::Unexist);

        // remove old position
        if let Some(x) = self.children.get_mut(&old_parent) {
            x.retain(|_, v| v != &target)
        }

        if !parent.is_deleted() {
            self.children
                .entry(parent)
                .or_default()
                .insert(NodePosition::new(position.clone().unwrap(), id), target);
        }

        self.trees.insert(
            target,
            TreeStateNode {
                parent,
                position,
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

    pub fn contains_internal(&self, target: &TreeID) -> bool {
        self.trees.contains_key(target)
    }

    /// Get the parent of the node, if the node is deleted or does not exist, return None
    pub fn parent(&self, target: TreeID) -> TreeParentId {
        self.trees
            .get(&target)
            .map(|x| x.parent)
            .unwrap_or(TreeParentId::Unexist)
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
        // ans
        self.children
            .get(parent)
            .map(|x| x.values().copied().collect())
            .unwrap_or_default()
    }

    pub fn children_num(&self, parent: &TreeParentId) -> Option<usize> {
        self.children.get(parent).map(|x| x.len())
    }

    pub fn is_parent(&self, parent: &TreeParentId, target: &TreeID) -> bool {
        self.trees
            .get(target)
            .map_or(false, |x| x.parent == *parent)
    }

    pub(crate) fn delete_position(&mut self, parent: &TreeParentId, target: TreeID) {
        if let Some(x) = self.children.get_mut(parent) {
            x.retain(|_, v| v != &target);
        }
    }

    // TODO: correct
    //
    pub(crate) fn generate_position_at(
        &mut self,
        parent: &TreeParentId,
        index: usize,
    ) -> Result<FracIndex, Vec<TreeID>> {
        let mut same_position = vec![];
        {
            let mut left = None;
            let mut right = None;
            let children_positions = self.children.get(parent);
            if children_positions.is_none() {
                debug_assert_eq!(index, 0);
                return Ok(FracIndex::default());
            }
            let mut positions = children_positions.unwrap().iter();
            let children_num = positions.len();

            if index > 0 {
                left = Some(&positions.nth(index - 1).unwrap().0.position);
            }
            if index < children_num {
                let t = positions.next().unwrap();
                right = Some(t.0);
            }

            if left.is_some() && left == right.map(|x| &x.position) {
                // TODO: the min length between left and right
                same_position.push(right.unwrap().clone());
                for (p, _) in positions {
                    if p.position == right.unwrap().position {
                        same_position.push(p.clone());
                    } else {
                        break;
                    }
                }
            }

            if same_position.is_empty() {
                return Ok(FracIndex::new(left, right.map(|x| &x.position)).unwrap());
            }
        }
        Err(same_position
            .into_iter()
            .map(|x| self.children.get_mut(parent).unwrap().remove(&x).unwrap())
            .collect())
    }

    pub(crate) fn get_index_by_tree_id(
        &self,
        parent: &TreeParentId,
        target: &TreeID,
    ) -> Option<usize> {
        (!parent.is_deleted())
            .then(|| {
                self.children
                    .get(parent)
                    .and_then(|x| x.values().position(|x| x == target))
            })
            .flatten()
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
        let mut ans = vec![];
        if let InternalDiff::Tree(tree) = &diff {
            // assert never cause cycle move
            for diff in tree.diff.iter() {
                let last_move_op = diff.last_effective_move_op_id;
                let target = diff.target;
                // create associated metadata container
                match &diff.action {
                    TreeInternalDiff::Create { parent, position } => {
                        self.mov(target, *parent, last_move_op, Some(position.clone()), false)
                            .unwrap();
                        let index = self.get_index_by_tree_id(parent, &target).unwrap();
                        ans.push(TreeDiffItem {
                            target,
                            action: TreeExternalDiff::Create {
                                parent: parent.into_node().ok(),
                                index,
                                position: position.clone(),
                            },
                        });
                    }
                    TreeInternalDiff::Move { parent, position } => {
                        self.mov(target, *parent, last_move_op, Some(position.clone()), false)
                            .unwrap();
                        let index = self.get_index_by_tree_id(parent, &target).unwrap();
                        ans.push(TreeDiffItem {
                            target,
                            action: TreeExternalDiff::Move {
                                parent: parent.into_node().ok(),
                                index,
                                position: position.clone(),
                            },
                        });
                    }
                    TreeInternalDiff::Delete { parent, position } => {
                        self.mov(target, *parent, last_move_op, position.clone(), false)
                            .unwrap();
                        ans.push(TreeDiffItem {
                            target,
                            action: TreeExternalDiff::Delete,
                        });
                    }
                    TreeInternalDiff::MoveInDelete { parent, position } => {
                        self.mov(target, *parent, last_move_op, position.clone(), false)
                            .unwrap();
                    }
                    TreeInternalDiff::UnCreate => {
                        ans.push(TreeDiffItem {
                            target,
                            action: TreeExternalDiff::Delete,
                        });
                        // delete it from state
                        let parent = self.trees.remove(&target);
                        if let Some(parent) = parent {
                            // TODO: we cannot use NodePosition to delete the cache, because the op_id is not the id when the node is cached
                            if let Some(x) = self.children.get_mut(&parent.parent) {
                                x.retain(|_, v| v != &target);
                            }
                        }
                        continue;
                    }
                };
            }
        }

        Diff::Tree(TreeDiff { diff: ans })
    }

    fn apply_diff(
        &mut self,
        diff: InternalDiff,
        _arena: &SharedArena,
        _txn: &Weak<Mutex<Option<Transaction>>>,
        _state: &Weak<Mutex<DocState>>,
    ) {
        if let InternalDiff::Tree(tree) = &diff {
            // assert never cause cycle move
            for diff in tree.diff.iter() {
                let last_move_op = diff.last_effective_move_op_id;
                let target = diff.target;
                // create associated metadata container
                match &diff.action {
                    TreeInternalDiff::Create { parent, position }
                    | TreeInternalDiff::Move { parent, position } => {
                        self.mov(target, *parent, last_move_op, Some(position.clone()), false)
                            .unwrap();
                    }
                    TreeInternalDiff::Delete { parent, position } => {
                        self.mov(target, *parent, last_move_op, position.clone(), false)
                            .unwrap();
                    }
                    TreeInternalDiff::MoveInDelete { parent, position } => {
                        self.mov(target, *parent, last_move_op, position.clone(), false)
                            .unwrap();
                    }
                    TreeInternalDiff::UnCreate => {
                        // delete it from state
                        let parent = self.trees.remove(&target);
                        if let Some(parent) = parent {
                            self.children.get_mut(&parent.parent).map(|x| {
                                x.remove(&NodePosition::new(parent.position.unwrap(), last_move_op))
                            });
                        }
                        continue;
                    }
                };
            }
        }
    }

    fn apply_local_op(&mut self, raw_op: &RawOp, _op: &crate::op::Op) -> LoroResult<()> {
        match &raw_op.content {
            crate::op::RawOpContent::Tree(tree) => {
                let TreeOp {
                    target,
                    parent,
                    position,
                } = tree;
                // TODO: use TreeParentId
                let parent = match parent {
                    Some(parent) => {
                        if TreeID::is_deleted_root(parent) {
                            TreeParentId::Deleted
                        } else {
                            TreeParentId::Node(*parent)
                        }
                    }
                    None => TreeParentId::None,
                };
                self.mov(*target, parent, raw_op.id_full(), position.clone(), true)
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
        let Some(roots) = self.children.get(&TreeParentId::None) else {
            return Diff::Tree(TreeDiff { diff: vec![] });
        };

        let mut q = VecDeque::from_iter(roots.iter());
        let mut index = 0;
        let mut parent = TreeParentId::None;
        while let Some((position, node)) = q.pop_front() {
            let node_parent = self.trees.get(node).unwrap().parent;
            if node_parent != parent {
                index = 0;
                parent = node_parent;
            }
            let diff = TreeDiffItem {
                target: *node,
                action: TreeExternalDiff::Create {
                    parent: node_parent.into_node().ok(),
                    index,
                    position: position.position.clone(),
                },
            };
            index += 1;
            diffs.push(diff);
            if let Some(children) = self.children.get(&TreeParentId::Node(*node)) {
                q.extend(children.iter());
            }
        }

        Diff::Tree(TreeDiff { diff: diffs })
    }

    fn get_value(&mut self) -> LoroValue {
        let mut ans = vec![];
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
                #[cfg(feature = "test_utils")]
                t.insert(
                    "index".to_string(),
                    (self.get_index_by_tree_id(&node.parent, target).unwrap() as i64).into(),
                );
                #[cfg(feature = "test_utils")]
                t.insert(
                    "position".to_string(),
                    node.position.clone().unwrap().to_string().into(),
                );
                ans.push(t);
            }
        }
        #[cfg(feature = "test_utils")]
        ans.sort_by_key(|x| {
            let parent = if let LoroValue::String(p) = x.get("parent").unwrap() {
                Some(p.clone())
            } else {
                None
            };
            (parent, *x.get("index").unwrap().as_i64().unwrap())
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
        self.trees
            .keys()
            .map(|n| n.associated_meta_container())
            .collect_vec()
    }

    #[doc = " Get a list of ops that can be used to restore the state to the current state"]
    fn encode_snapshot(&self, mut encoder: StateSnapshotEncoder) -> Vec<u8> {
        // TODO: better
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
            let target = content.target;
            let parent = content.parent;
            let position = content.position.clone();
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
            self.mov(target, parent, op.id_full(), position, false)
                .unwrap();
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
