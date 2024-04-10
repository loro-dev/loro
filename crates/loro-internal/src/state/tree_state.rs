use either::Either;
use enum_as_inner::EnumAsInner;
use fractional_index::FractionalIndex;
use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::{
    ContainerID, IdFull, IdLp, LoroError, LoroResult, LoroTreeError, LoroValue, TreeID,
};
use rle::HasLength;
use serde::Serialize;
use std::collections::VecDeque;
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
    BTree(btree::ChildTree),
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
            NodeChildren::BTree(btree) => btree.id_to_index(target),
        }
    }

    fn get_node_position_at(&self, pos: usize) -> &NodePosition {
        match self {
            NodeChildren::Vec(v) => &v[pos].0,
            NodeChildren::BTree(btree) => &btree.get_elem_at(pos).unwrap().pos,
        }
    }

    fn get_elem_at(&self, pos: usize) -> Option<(&NodePosition, &TreeID)> {
        match self {
            NodeChildren::Vec(v) => v.get(pos).map(|(pos, id)| (pos, id)),
            NodeChildren::BTree(btree) => btree.get_elem_at(pos).map(|x| (x.pos.as_ref(), &x.id)),
        }
    }

    fn generate_fi_at(&self, pos: usize, target: &TreeID) -> FractionalIndexGenResult {
        let mut reset_ids = vec![];
        let mut left = None;
        let mut next_right = None;
        {
            let mut right = None;
            let children_num = self.len();

            if pos > 0 {
                left = Some(self.get_node_position_at(pos - 1));
            }
            if pos < children_num {
                right = self.get_elem_at(pos);
            }

            let left_fi = left.map(|x| &x.position);
            // if left and right have the same fractional indexes, we need to scan further to
            // find all the ids that need to be reset
            if let Some(left_fi) = left_fi {
                if Some(left_fi) == right.map(|x| &x.0.position) {
                    // TODO: the min length between left and right
                    reset_ids.push(*right.unwrap().1);
                    for i in (pos + 1)..children_num {
                        let this_position = &self.get_node_position_at(i).position;
                        if this_position == left_fi {
                            reset_ids.push(*self.get_elem_at(i).unwrap().1);
                        } else {
                            next_right = Some(this_position.clone());
                            break;
                        }
                    }
                }
            }

            if reset_ids.is_empty() {
                return FractionalIndexGenResult::Ok(
                    FractionalIndex::new(left.map(|x| &x.position), right.map(|x| &x.0.position))
                        .unwrap(),
                );
            }
        }
        let positions = FractionalIndex::generate_n_evenly(
            left.map(|x| &x.position),
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

    fn get_id_at(&self, pos: usize) -> &TreeID {
        match self {
            NodeChildren::Vec(v) => &v[pos].1,
            NodeChildren::BTree(btree) => &btree.get_elem_at(pos).unwrap().id,
        }
    }

    fn delete_child(&mut self, target: &TreeID) {
        match self {
            NodeChildren::Vec(v) => {
                v.retain(|(_, id)| id != target);
            }
            NodeChildren::BTree(v) => {
                v.delete_child(target);
            }
        }
    }

    fn upgrade(&mut self) {
        match self {
            NodeChildren::Vec(v) => {
                let mut btree = btree::ChildTree::new();
                for (pos, id) in v.drain(..) {
                    btree.insert_child(pos, id);
                }
                *self = NodeChildren::BTree(btree);
            }
            NodeChildren::BTree(_) => unreachable!(),
        }
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
            NodeChildren::BTree(v) => {
                v.insert_child(pos, id);
            }
        }
    }

    fn len(&self) -> usize {
        match self {
            NodeChildren::Vec(v) => v.len(),
            NodeChildren::BTree(v) => v.len(),
        }
    }

    fn has_child(&self, node_position: &NodePosition) -> bool {
        match self {
            NodeChildren::Vec(v) => v
                .binary_search_by(|(target, _)| target.cmp(&node_position))
                .is_ok(),
            NodeChildren::BTree(v) => v.has_child(node_position),
        }
    }

    fn iter(&self) -> impl Iterator<Item = (&NodePosition, &TreeID)> {
        match self {
            NodeChildren::Vec(v) => Either::Left(v.iter().map(|(pos, id)| (pos, id))),
            NodeChildren::BTree(t) => Either::Right(t.iter()),
        }
    }
}

#[derive(Clone, Default)]
struct TreeChildrenCache(FxHashMap<TreeParentId, NodeChildren>);

mod btree {
    use std::{cmp::Ordering, ops::Range, sync::Arc};

    use fxhash::FxHashMap;
    use generic_btree::{
        rle::{HasLength, Mergeable, Sliceable},
        BTree, BTreeTrait, Cursor, FindResult, LeafIndex, LengthFinder, Query, UseLengthFinder,
    };
    use loro_common::TreeID;

    use super::NodePosition;

    struct ChildTreeTrait;
    #[derive(Debug, Clone)]
    pub(super) struct ChildTree {
        tree: BTree<ChildTreeTrait>,
        id_to_leaf_index: FxHashMap<TreeID, LeafIndex>,
    }

    impl ChildTree {
        pub(super) fn new() -> Self {
            Self {
                tree: BTree::new(),
                id_to_leaf_index: FxHashMap::default(),
            }
        }

        pub(super) fn insert_child(&mut self, pos: NodePosition, id: TreeID) {
            let (c, _) = self.tree.insert::<KeyQuery>(
                &pos,
                Elem {
                    pos: Arc::new(pos.clone()),
                    id,
                },
            );

            self.id_to_leaf_index.insert(id, c.leaf);
        }

        pub(super) fn delete_child(&mut self, id: &TreeID) {
            if let Some(leaf) = self.id_to_leaf_index.remove(id) {
                self.tree.remove_leaf(Cursor { leaf, offset: 0 });
            } else {
                panic!("The id is not in the tree");
            }
        }

        pub(super) fn has_child(&self, pos: &NodePosition) -> bool {
            match self.tree.query::<KeyQuery>(pos) {
                Some(r) => r.found,
                None => false,
            }
        }

        pub(super) fn iter(&self) -> impl Iterator<Item = (&NodePosition, &TreeID)> {
            self.tree.iter().map(|x| (&*x.pos, &x.id))
        }

        pub(super) fn len(&self) -> usize {
            self.tree.root_cache().len
        }

        pub(super) fn get_elem_at(&self, pos: usize) -> Option<&Elem> {
            let result = self.tree.query::<LengthFinder>(&pos)?;
            if !result.found {
                return None;
            }
            self.tree.get_elem(result.leaf())
        }

        pub(super) fn id_to_index(&self, id: &TreeID) -> Option<usize> {
            let leaf_index = self.id_to_leaf_index.get(id)?;
            let mut ans = 0;
            self.tree.visit_previous_caches(
                Cursor {
                    leaf: *leaf_index,
                    offset: 0,
                },
                |prev| match prev {
                    generic_btree::PreviousCache::NodeCache(c) => {
                        ans += c.len;
                    }
                    generic_btree::PreviousCache::PrevSiblingElem(p) => {
                        ans += 1;
                    }
                    generic_btree::PreviousCache::ThisElemAndOffset { .. } => {}
                },
            );

            Some(ans)
        }
    }

    #[derive(Clone, Debug)]
    pub(super) struct Elem {
        pub(super) pos: Arc<NodePosition>,
        pub(super) id: TreeID,
    }

    impl Mergeable for Elem {
        fn can_merge(&self, rhs: &Self) -> bool {
            false
        }

        fn merge_right(&mut self, rhs: &Self) {
            unreachable!()
        }

        fn merge_left(&mut self, left: &Self) {
            unreachable!()
        }
    }

    impl HasLength for Elem {
        fn rle_len(&self) -> usize {
            1
        }
    }

    impl Sliceable for Elem {
        fn _slice(&self, range: std::ops::Range<usize>) -> Self {
            assert!(range.len() == 1);
            self.clone()
        }
    }

    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    struct Cache {
        range: Option<Range<Arc<NodePosition>>>,
        len: usize,
    }

    impl BTreeTrait for ChildTreeTrait {
        type Elem = Elem;
        type Cache = Cache;
        type CacheDiff = ();
        const USE_DIFF: bool = false;

        fn calc_cache_internal(
            cache: &mut Self::Cache,
            caches: &[generic_btree::Child<Self>],
        ) -> Self::CacheDiff {
            *cache = Cache {
                range: Some(
                    caches[0].cache.range.as_ref().unwrap().start.clone()
                        ..caches
                            .last()
                            .unwrap()
                            .cache
                            .range
                            .as_ref()
                            .unwrap()
                            .end
                            .clone(),
                ),
                len: caches.iter().map(|x| x.cache.len).sum(),
            };
        }

        fn apply_cache_diff(cache: &mut Self::Cache, diff: &Self::CacheDiff) {
            unreachable!()
        }

        fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {}

        fn get_elem_cache(elem: &Self::Elem) -> Self::Cache {
            Cache {
                range: Some(elem.pos.clone()..elem.pos.clone()),
                len: 1,
            }
        }

        fn new_cache_to_diff(cache: &Self::Cache) -> Self::CacheDiff {}

        fn sub_cache(cache_lhs: &Self::Cache, cache_rhs: &Self::Cache) -> Self::CacheDiff {}
    }

    struct KeyQuery;

    impl Query<ChildTreeTrait> for KeyQuery {
        type QueryArg = NodePosition;

        #[inline(always)]
        fn init(_target: &Self::QueryArg) -> Self {
            KeyQuery
        }

        #[inline]
        fn find_node(
            &mut self,
            target: &Self::QueryArg,
            caches: &[generic_btree::Child<ChildTreeTrait>],
        ) -> FindResult {
            match caches.binary_search_by(|x| {
                let range = x.cache.range.as_ref().unwrap();
                if target < &range.start {
                    core::cmp::Ordering::Greater
                } else if target > &range.end {
                    core::cmp::Ordering::Less
                } else {
                    core::cmp::Ordering::Equal
                }
            }) {
                Ok(i) => FindResult::new_found(i, 0),
                Err(i) => FindResult::new_missing(
                    i.min(caches.len() - 1),
                    if i == caches.len() { 1 } else { 0 },
                ),
            }
        }

        #[inline(always)]
        fn confirm_elem(
            &mut self,
            q: &Self::QueryArg,
            elem: &<ChildTreeTrait as BTreeTrait>::Elem,
        ) -> (usize, bool) {
            match q.cmp(&elem.pos) {
                Ordering::Less => (0, false),
                Ordering::Equal => (0, true),
                Ordering::Greater => (1, false),
            }
        }
    }

    impl UseLengthFinder<ChildTreeTrait> for ChildTreeTrait {
        fn get_len(cache: &<ChildTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.len
        }
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
        self.children.get(parent).map(|x| x.iter().map(|x| *x.1))
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
