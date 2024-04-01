use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::{ContainerID, IdFull, LoroError, LoroResult, LoroTreeError, LoroValue, TreeID};
use rle::HasLength;
use serde::{Deserialize, Serialize};
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
    pub(crate) trees: FxHashMap<TreeID, TreeStateNode>,
    pub(crate) children: FxHashMap<TreeParentId, BTreeMap<FracIndex, TreeID>>,
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
        if parent.is_none() {
            // new root node
            self.children
                .entry(parent)
                .or_default()
                .insert(position.clone().unwrap(), target);
            self.trees.insert(
                target,
                TreeStateNode {
                    parent,
                    position,
                    last_move_op: id,
                },
            );
            return Ok(());
        };
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
        let (old_parent, old_position) = self
            .trees
            .get(&target)
            .map(|x| (x.parent, x.position.clone()))
            .unwrap_or((TreeParentId::Unexist, None));

        // remove old position
        if let Some(old_position) = old_position {
            self.children
                .get_mut(&old_parent)
                .map(|x| x.remove(&old_position));
        }

        if !parent.is_deleted() {
            self.children
                .entry(parent)
                .or_default()
                .insert(position.clone().unwrap(), target);
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
        let mut ans = Vec::new();
        for (t, p) in self.trees.iter() {
            if &p.parent == parent {
                ans.push(*t);
            }
        }
        ans
    }

    // TODO: correct
    //
    pub fn generate_position_at(&self, parent: &TreeParentId, index: usize) -> FracIndex {
        let children_positions = self.children.get(parent);
        if let Some(positions) = children_positions {
            let mut positions = positions.keys();
            let children_num = positions.len();
            let mut left = None;
            let mut right = None;
            if index > 0 {
                left = Some(positions.nth(index - 1).unwrap());
            }
            if index < children_num {
                right = Some(positions.next().unwrap());
            }
            FracIndex::new(left, right).unwrap()
        } else {
            debug_assert_eq!(index, 0);
            FracIndex::default()
        }
    }

    fn check_new_position(
        &self,
        parent: &TreeParentId,
        position: &FracIndex,
        op_id: &IdFull,
    ) -> FracIndex {
        let mut position = position.clone();
        let children = self.children.get(parent);
        // has same position
        while let Some(conflict_node) = children.and_then(|x| x.get(&position)) {
            let conflict_id = self.trees.get(conflict_node).unwrap().last_move_op;
            let left = op_id.peer < conflict_id.peer;
            position = if left {
                let left_position = children
                    .unwrap()
                    .range(..&position)
                    .next_back()
                    .map(|x| x.0);
                FracIndex::new(left_position, Some(&position)).unwrap()
            } else {
                // TODO: Excluded
                let right_position = children.unwrap().range(&position..).nth(1).map(|x| x.0);
                FracIndex::new(Some(&position), right_position).unwrap()
            }
        }
        position
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
                        let position = self.check_new_position(parent, position, &last_move_op);
                        self.mov(target, *parent, last_move_op, Some(position.clone()), false)
                            .unwrap();
                        ans.push(TreeDiffItem {
                            target,
                            action: TreeExternalDiff::Create {
                                parent: parent.into_node().ok(),
                                position,
                            },
                        });
                    }
                    TreeInternalDiff::Move { parent, position } => {
                        let position = self.check_new_position(parent, position, &last_move_op);
                        self.mov(target, *parent, last_move_op, Some(position.clone()), false)
                            .unwrap();
                        ans.push(TreeDiffItem {
                            target,
                            action: TreeExternalDiff::Move {
                                parent: parent.into_node().ok(),
                                position,
                            },
                        });
                    }
                    TreeInternalDiff::Delete { parent } => {
                        self.mov(target, *parent, last_move_op, None, false)
                            .unwrap();
                        ans.push(TreeDiffItem {
                            target,
                            action: TreeExternalDiff::Delete,
                        });
                    }
                    TreeInternalDiff::MoveInDelete { parent, position } => {
                        let position = position
                            .as_ref()
                            .map(|p| self.check_new_position(parent, p, &last_move_op));
                        self.mov(target, *parent, last_move_op, position, false)
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
                            self.children
                                .get_mut(&parent.parent)
                                .map(|x| x.remove(&parent.position.unwrap()));
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
                        let position = self.check_new_position(parent, position, &last_move_op);
                        self.mov(target, *parent, last_move_op, Some(position.clone()), false)
                            .unwrap();
                    }
                    TreeInternalDiff::Delete { parent } => {
                        self.mov(target, *parent, last_move_op, None, false)
                            .unwrap();
                    }
                    TreeInternalDiff::MoveInDelete { parent, position } => {
                        let position = position
                            .as_ref()
                            .map(|p| self.check_new_position(parent, p, &last_move_op));
                        self.mov(target, *parent, last_move_op, position, false)
                            .unwrap();
                    }
                    TreeInternalDiff::UnCreate => {
                        // delete it from state
                        let parent = self.trees.remove(&target);
                        if let Some(parent) = parent {
                            self.children
                                .get_mut(&parent.parent)
                                .map(|x| x.remove(&parent.position.unwrap()));
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
        // TODO: perf
        let forest = Forest::from_tree_state(&self.trees);
        if forest.roots.is_empty() {
            return Diff::Tree(TreeDiff { diff: vec![] });
        }
        let mut q = VecDeque::from(forest.roots);

        while let Some(node) = q.pop_front() {
            let parent = if let Some(p) = node.parent {
                TreeParentId::Node(p)
            } else {
                TreeParentId::None
            };
            let diff = TreeDiffItem {
                target: node.id,
                action: TreeExternalDiff::Create {
                    parent: parent.into_node().ok(),
                    position: node.position.clone(),
                },
            };
            diffs.push(diff);
            q.extend(
                node.children
                    .into_iter()
                    .sorted_by_key(|x| x.position.clone()),
            );
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
                // TODO: better index
                t.insert(
                    "position".to_string(),
                    node.position.as_ref().unwrap().to_string().into(),
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
            (
                parent,
                x.get("position").unwrap().as_string().unwrap().clone(),
            )
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
        let mut ans = vec![];
        // TODO: better
        for node in self
            .trees
            .values()
            .sorted_by_key(|node| (node.last_move_op.lamport, node.last_move_op.peer))
        {
            if node.last_move_op == IdFull::NONE_ID {
                continue;
            }
            encoder.encode_op(node.last_move_op.idlp().into(), || unimplemented!());
            let position_bytes = node.position.as_ref().map(|x| x.as_bytes());
            ans.push(position_bytes);
        }

        postcard::to_allocvec(&ans).unwrap()
    }

    #[doc = " Restore the state to the state represented by the ops that exported by `get_snapshot_ops`"]
    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext) {
        assert_eq!(ctx.mode, EncodeMode::Snapshot);
        let positions: Vec<Option<Vec<u8>>> = postcard::from_bytes(ctx.blob).unwrap();
        for (op, position) in ctx.ops.zip(positions) {
            assert_eq!(op.op.atom_len(), 1);
            let content = op.op.content.as_tree().unwrap();
            let target = content.target;
            let parent = content.parent;
            let position = position.map(|x| FracIndex::from_bytes(x).unwrap());
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
            // if let Some(p) = position {
            //     position = Some(self.check_new_position(&parent, &p, &op.id_full()));
            // }
            self.mov(target, parent, op.id_full(), position, false)
                .unwrap();
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
    position: FracIndex,
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
                if TreeID::is_deleted_root(&root) {
                    continue;
                }
                let position = state.get(&root).unwrap().position.clone();
                let mut stack = vec![(
                    root,
                    TreeNode {
                        id: root,
                        parent: None,
                        position: position.unwrap(),
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
                                        position: state
                                            .get(child)
                                            .unwrap()
                                            .position
                                            .clone()
                                            .unwrap(),
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
