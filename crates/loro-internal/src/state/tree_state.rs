use enum_as_inner::EnumAsInner;
use fractional_index::FractionalIndex;
use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::{
    ContainerID, IdFull, IdLp, LoroError, LoroResult, LoroTreeError, LoroValue, TreeID,
};
use rle::HasLength;
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
use std::fmt::Debug;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, Weak};

use crate::container::idx::ContainerIdx;
use crate::delta::{TreeDiff, TreeDiffItem, TreeExternalDiff};
use crate::encoding::{EncodeMode, StateSnapshotDecodeContext, StateSnapshotEncoder};
use crate::event::InternalDiff;
use crate::op::Op;
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
    // We use `Unexist` as the old parent of a new node created
    // so we can infer the retreat internal diff is `Uncreate`
    Unexist,
    Deleted,
    Root,
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
            None => TreeParentId::Root,
        }
    }
}

#[derive(Clone)]
enum NodeChildren {
    Vec(Vec<(NodePosition, TreeID)>),
}

impl Default for NodeChildren {
    fn default() -> Self {
        NodeChildren::Vec(vec![])
    }
}

impl NodeChildren {
    fn get_index_by_child_id(&self, target: &TreeID) -> Option<usize> {
        match self {
            NodeChildren::Vec(v) => v.iter().position(|(_, id)| id == target),
        }
    }

    fn get_fi_at(&self, pos: usize) -> &FractionalIndex {
        match self {
            NodeChildren::Vec(v) => &v[pos].0.position,
        }
    }

    fn generate_fi_at(&self, pos: usize, target: &TreeID) -> FractionalIndexGenResult {
        match self {
            NodeChildren::Vec(v) => {
                let mut reset_ids = vec![];
                let mut left = None;
                let mut next_right = None;
                {
                    let mut right = None;
                    let children_num = v.len();

                    if pos > 0 {
                        left = Some(&v[pos - 1].0.position);
                    }
                    if pos < children_num {
                        right = Some(&v[pos]);
                    }

                    // if left and right have the same fractional indexes, we need to scan further to
                    // find all the ids that need to be reset
                    if left.is_some() && left == right.map(|x| &x.0.position) {
                        // TODO: the min length between left and right
                        reset_ids.push(right.unwrap().1);
                        for i in (pos + 1)..children_num {
                            if &v[i].0.position == right.map(|x| &x.0.position).unwrap() {
                                reset_ids.push(v[i].1);
                            } else {
                                next_right = Some(v[i].0.position.clone());

                                break;
                            }
                        }
                    }

                    if reset_ids.is_empty() {
                        return FractionalIndexGenResult::Ok(
                            FractionalIndex::new(left, right.map(|x| &x.0.position)).unwrap(),
                        );
                    }
                }
                let positions = FractionalIndex::generate_n_evenly(
                    left,
                    next_right.as_ref(),
                    reset_ids.len() + 1,
                )
                .unwrap();
                FractionalIndexGenResult::Rearrange(
                    Some(*target)
                        .into_iter()
                        .chain(reset_ids)
                        .zip(positions)
                        .collect(),
                )
            }
        }
    }

    fn get_id_at(&self, pos: usize) -> &TreeID {
        match self {
            NodeChildren::Vec(v) => &v[pos].1,
        }
    }

    fn delete_child(&mut self, target: &TreeID) {
        match self {
            NodeChildren::Vec(v) => {
                v.retain(|(_, id)| id != target);
            }
        }
    }

    fn upgrade(&mut self) {
        unimplemented!("Upgrade the vec to a BTreeMap")
    }

    fn insert_child(&mut self, pos: NodePosition, id: TreeID) {
        match self {
            NodeChildren::Vec(v) => {
                if v.len() >= 16 {
                    self.upgrade();
                    return self.insert_child(pos, id);
                }

                let r = v.binary_search_by(|(target, _)| target.cmp(&pos));
                match r {
                    Ok(_) => unreachable!(),
                    Err(i) => {
                        v.insert(i, (pos, id));
                    }
                }
            }
        }
    }

    fn len(&self) -> usize {
        match self {
            NodeChildren::Vec(v) => v.len(),
        }
    }

    fn has_child(&self, node_position: &NodePosition) -> bool {
        match self {
            NodeChildren::Vec(v) => v
                .binary_search_by(|(target, _)| target.cmp(&node_position))
                .is_ok(),
        }
    }

    fn iter(&self) -> impl Iterator<Item = &(NodePosition, TreeID)> {
        match self {
            NodeChildren::Vec(v) => v.iter(),
        }
    }
}

#[derive(Clone, Default)]
struct TreeChildrenCache(FxHashMap<TreeParentId, NodeChildren>);

mod btree {
    use fxhash::FxHashMap;
    use generic_btree::LeafIndex;
    use loro_common::{IdLp, ID};

    struct ChildTree {
        id_to_leaf_index: FxHashMap<ID, LeafIndex>,
    }
}

impl Debug for TreeChildrenCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "TreeChildrenCache {{")?;
        for (parent, children) in self.0.iter() {
            writeln!(f, "  {:?}:{{", parent)?;
            for (position, id) in children.iter() {
                writeln!(f, "      {:?} -> {:?}", position, id)?;
            }
            writeln!(f, "  }}")?;
        }
        writeln!(f, "}}")
    }
}

impl Deref for TreeChildrenCache {
    type Target = FxHashMap<TreeParentId, NodeChildren>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for TreeChildrenCache {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// The state of movable tree.
///
/// using flat representation
#[derive(Debug, Clone)]
pub struct TreeState {
    idx: ContainerIdx,
    trees: FxHashMap<TreeID, TreeStateNode>,
    // TODO: PERF BTreeMap can be replaced by a generic_btree::BTree
    children: TreeChildrenCache,
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord)]
struct NodePosition {
    position: FractionalIndex,
    // different nodes created by a peer may have the same position
    // when we merge updates that cause cycles.
    // for example [::fuzz::test::test_tree::same_peer_have_same_position()]
    idlp: IdLp,
}

impl NodePosition {
    fn new(position: FractionalIndex, idlp: IdLp) -> Self {
        Self { position, idlp }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeStateNode {
    pub parent: TreeParentId,
    // no position in delete?
    pub position: Option<FractionalIndex>,
    pub last_move_op: IdFull,
}

impl TreeState {
    pub fn new(idx: ContainerIdx) -> Self {
        Self {
            idx,
            trees: FxHashMap::default(),
            children: Default::default(),
        }
    }

    pub fn mov(
        &mut self,
        target: TreeID,
        parent: TreeParentId,
        id: IdFull,
        position: Option<FractionalIndex>,
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
        if let Some(old_parent) = self.trees.get(&target).map(|x| x.parent) {
            // remove old position
            self.delete_position(&old_parent, target);
        }

        if !parent.is_deleted() {
            let entry = self.children.entry(parent).or_default();
            let node_position = NodePosition::new(position.clone().unwrap(), id.idlp());
            debug_assert!(!entry.has_child(&node_position));
            entry.insert_child(node_position, target);
        } else {
            // clean the cache recursively, otherwise the index of event will be calculated incorrectly
            let mut q = vec![target];
            while let Some(id) = q.pop() {
                let parent = TreeParentId::from(Some(id));
                if let Some(children) = self.children.get(&parent) {
                    q.extend(children.iter().map(|x| x.1));
                }
                self.children.remove(&parent);
            }
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
            TreeParentId::Deleted | TreeParentId::Root => false,
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
                TreeParentId::Root => false,
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

    pub fn get_children(&self, parent: &TreeParentId) -> Option<impl Iterator<Item = TreeID> + '_> {
        self.children.get(parent).map(|x| x.iter().map(|x| x.1))
    }

    pub fn children_num(&self, parent: &TreeParentId) -> Option<usize> {
        self.children.get(parent).map(|x| x.len())
    }

    /// Determine whether the target is the child of the node
    ///
    /// O(1)
    pub fn is_parent(&self, parent: &TreeParentId, target: &TreeID) -> bool {
        self.trees
            .get(target)
            .map_or(false, |x| x.parent == *parent)
    }

    /// Delete the position cache of the node
    ///
    /// O(1) + Clone FractionalIndex
    pub(crate) fn delete_position(&mut self, parent: &TreeParentId, target: TreeID) {
        if let Some(x) = self.children.get_mut(parent) {
            x.delete_child(&target);
        }
    }

    pub(crate) fn generate_position_at(
        &mut self,
        target: &TreeID,
        parent: &TreeParentId,
        index: usize,
    ) -> FractionalIndexGenResult {
        self.children
            .entry(*parent)
            .or_default()
            .generate_fi_at(index, target)
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
                    // TODO: PERF: Slow
                    .and_then(|x| x.get_index_by_child_id(target))
            })
            .flatten()
    }
}

pub(crate) enum FractionalIndexGenResult {
    Ok(FractionalIndex),
    Rearrange(Vec<(TreeID, FractionalIndex)>),
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
            // println!("before {:?}", self.children);
            // assert never cause cycle move
            for diff in tree.diff.iter() {
                // println!("\ndiff {:?}", diff);
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
                            if !parent.parent.is_deleted() {
                                let node_position = NodePosition::new(
                                    parent.position.unwrap(),
                                    parent.last_move_op.idlp(),
                                );
                                self.children
                                    .get_mut(&parent.parent)
                                    .unwrap()
                                    .delete_child(&target);
                            }
                        }
                        // println!("after {:?}", self.children);
                        continue;
                    }
                };
                // println!("after {:?}", self.children);
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
                            if !parent.parent.is_deleted() {
                                self.children
                                    .get_mut(&parent.parent)
                                    .unwrap()
                                    .delete_child(&target);
                            }
                        }
                        continue;
                    }
                };
            }
        }
    }

    fn apply_local_op(&mut self, raw_op: &RawOp, _op: &Op) -> LoroResult<()> {
        match &raw_op.content {
            crate::op::RawOpContent::Tree(tree) => {
                let TreeOp {
                    target,
                    parent,
                    position,
                } = tree;
                let parent = TreeParentId::from(*parent);
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
        let Some(roots) = self.children.get(&TreeParentId::Root) else {
            return Diff::Tree(TreeDiff { diff: vec![] });
        };

        let mut q = VecDeque::from_iter(roots.iter());
        let mut index = 0;
        let mut parent = TreeParentId::Root;
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
                // TODO: Refactor: you can include the index and parent in the q
                // The code will be more robust and easy to understand
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
                t.insert(
                    "index".to_string(),
                    (self.get_index_by_tree_id(&node.parent, target).unwrap() as i64).into(),
                );
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
            let parent = TreeParentId::from(parent);
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
