use either::Either;
use enum_as_inner::EnumAsInner;
use fractional_index::FractionalIndex;
use fxhash::{FxHashMap, FxHashSet};
use itertools::Itertools;
use loro_common::{
    ContainerID, IdFull, IdLp, LoroError, LoroResult, LoroTreeError, LoroValue, PeerID, TreeID,
    DELETED_TREE_ROOT, ID,
};
use rand::SeedableRng;
use rle::HasLength;
use serde::Serialize;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::ops::{Deref, DerefMut};
use std::sync::{Mutex, Weak};

use super::{ApplyLocalOpReturn, ContainerState, DiffApplyContext};
use crate::configure::Configure;
use crate::container::idx::ContainerIdx;
use crate::delta::{TreeDiff, TreeDiffItem, TreeExternalDiff};
use crate::diff_calc::DiffMode;
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

#[derive(Clone, Debug, Default, EnumAsInner)]
pub enum TreeFractionalIndexConfigInner {
    GenerateFractionalIndex {
        jitter: u8,
        rng: Box<rand::rngs::StdRng>,
    },
    #[default]
    AlwaysDefault,
}

/// The state of movable tree.
///
/// using flat representation
#[derive(Debug, Clone)]
pub struct TreeState {
    idx: ContainerIdx,
    trees: FxHashMap<TreeID, TreeStateNode>,
    children: TreeChildrenCache,
    fractional_index_config: TreeFractionalIndexConfigInner,
    peer_id: PeerID,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeStateNode {
    pub parent: TreeParentId,
    pub position: Option<FractionalIndex>,
    pub last_move_op: IdFull,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct NodePosition {
    pub(crate) position: FractionalIndex,
    // different nodes created by a peer may have the same position
    // when we merge updates that cause cycles.
    // for example [::fuzz::test::test_tree::same_peer_have_same_position()]
    pub(crate) idlp: IdLp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumAsInner, Serialize)]
pub enum TreeParentId {
    Node(TreeID),
    Root,
    Deleted,
    // We use `Unexist` as the old parent of a new node created
    // so we can infer the retreat internal diff is `Uncreate`
    Unexist,
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

impl From<TreeID> for TreeParentId {
    fn from(id: TreeID) -> Self {
        if TreeID::is_deleted_root(&id) {
            TreeParentId::Deleted
        } else {
            TreeParentId::Node(id)
        }
    }
}

impl From<&TreeID> for TreeParentId {
    fn from(id: &TreeID) -> Self {
        if TreeID::is_deleted_root(id) {
            TreeParentId::Deleted
        } else {
            TreeParentId::Node(*id)
        }
    }
}

impl TreeParentId {
    pub fn tree_id(&self) -> Option<TreeID> {
        match self {
            TreeParentId::Node(id) => Some(*id),
            TreeParentId::Root => None,
            TreeParentId::Deleted => Some(DELETED_TREE_ROOT),
            TreeParentId::Unexist => unreachable!(),
        }
    }
}

#[derive(Debug, Clone)]
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
    const MAX_SIZE_FOR_ARRAY: usize = 16;
    fn get_index_by_child_id(&self, target: &TreeID) -> Option<usize> {
        match self {
            NodeChildren::Vec(v) => v.iter().position(|(_, id)| id == target),
            NodeChildren::BTree(btree) => btree.id_to_index(target),
        }
    }

    fn get_last_insert_index_by_position(
        &self,
        node_position: &NodePosition,
    ) -> Result<usize, usize> {
        match self {
            NodeChildren::Vec(v) => v.binary_search_by_key(&node_position, |x| &x.0),
            NodeChildren::BTree(btree) => btree.get_index_by_node_position(node_position),
        }
    }

    fn get_node_position_at(&self, pos: usize) -> Option<&NodePosition> {
        match self {
            NodeChildren::Vec(v) => v.get(pos).map(|x| &x.0),
            NodeChildren::BTree(btree) => btree.get_elem_at(pos).map(|x| x.pos.as_ref()),
        }
    }

    fn get_elem_at(&self, pos: usize) -> Option<(&NodePosition, &TreeID)> {
        match self {
            NodeChildren::Vec(v) => v.get(pos).map(|x| (&x.0, &x.1)),
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
            if children_num == 0 {
                return FractionalIndexGenResult::Ok(FractionalIndex::default());
            }

            if pos > 0 {
                left = self.get_node_position_at(pos - 1);
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
                        let this_position =
                            self.get_node_position_at(i).map(|x| &x.position).unwrap();
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

    fn get_id_at(&self, pos: usize) -> Option<TreeID> {
        match self {
            NodeChildren::Vec(v) => v.get(pos).map(|x| x.1),
            NodeChildren::BTree(btree) => btree.get_elem_at(pos).map(|x| x.id),
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
                if v.len() >= Self::MAX_SIZE_FOR_ARRAY {
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

    fn push_child_in_order(&mut self, pos: NodePosition, id: TreeID) {
        match self {
            NodeChildren::Vec(v) => {
                if v.len() >= Self::MAX_SIZE_FOR_ARRAY {
                    self.upgrade();
                    return self.push_child_in_order(pos, id);
                }

                if let Some(last) = v.last() {
                    assert!(last.0 < pos);
                }
                v.push((pos, id));
            }
            NodeChildren::BTree(v) => {
                v.push_child_in_order(pos, id);
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
                .binary_search_by(|(target, _)| target.cmp(node_position))
                .is_ok(),
            NodeChildren::BTree(v) => v.has_child(node_position),
        }
    }

    fn iter(&self) -> impl Iterator<Item = (&NodePosition, &TreeID)> {
        match self {
            NodeChildren::Vec(v) => Either::Left(v.iter().map(|x| (&x.0, &x.1))),
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
        rle::{CanRemove, HasLength, Mergeable, Sliceable, TryInsert},
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

        pub(super) fn push_child_in_order(&mut self, pos: NodePosition, id: TreeID) {
            let c = self.tree.push(Elem {
                pos: Arc::new(pos.clone()),
                id,
            });
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
                    generic_btree::PreviousCache::PrevSiblingElem(_) => {
                        ans += 1;
                    }
                    generic_btree::PreviousCache::ThisElemAndOffset { .. } => {}
                },
            );

            Some(ans)
        }

        pub(super) fn get_index_by_node_position(
            &self,
            node_position: &NodePosition,
        ) -> Result<usize, usize> {
            let Some(res) = self.tree.query::<KeyQuery>(node_position) else {
                return Ok(0);
            };
            let mut ans = 0;
            self.tree
                .visit_previous_caches(res.cursor, |prev| match prev {
                    generic_btree::PreviousCache::NodeCache(c) => {
                        ans += c.len;
                    }
                    generic_btree::PreviousCache::PrevSiblingElem(_) => {
                        ans += 1;
                    }
                    generic_btree::PreviousCache::ThisElemAndOffset { elem: _, offset } => {
                        ans += offset;
                    }
                });
            if res.found {
                Ok(ans)
            } else {
                Err(ans)
            }
        }
    }

    #[derive(Clone, Debug)]
    pub(super) struct Elem {
        pub(super) pos: Arc<NodePosition>,
        pub(super) id: TreeID,
    }

    impl Mergeable for Elem {
        fn can_merge(&self, _rhs: &Self) -> bool {
            false
        }

        fn merge_right(&mut self, _rhs: &Self) {
            unreachable!()
        }

        fn merge_left(&mut self, _left: &Self) {
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

    impl CanRemove for Elem {
        fn can_remove(&self) -> bool {
            false
        }
    }

    impl TryInsert for Elem {
        fn try_insert(&mut self, _pos: usize, elem: Self) -> Result<(), Self>
        where
            Self: Sized,
        {
            Err(elem)
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
            if caches.is_empty() {
                *cache = Default::default();
                return;
            }

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

        fn apply_cache_diff(_cache: &mut Self::Cache, _diff: &Self::CacheDiff) {
            unreachable!()
        }

        fn merge_cache_diff(_diff1: &mut Self::CacheDiff, _diff2: &Self::CacheDiff) {}

        fn get_elem_cache(elem: &Self::Elem) -> Self::Cache {
            Cache {
                range: Some(elem.pos.clone()..elem.pos.clone()),
                len: 1,
            }
        }

        fn new_cache_to_diff(_cache: &Self::Cache) -> Self::CacheDiff {}

        fn sub_cache(_cache_lhs: &Self::Cache, _cache_rhs: &Self::Cache) -> Self::CacheDiff {}
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
            let result = caches.binary_search_by(|x| {
                let range = x.cache.range.as_ref().unwrap();
                if target < &range.start {
                    core::cmp::Ordering::Greater
                } else if target > &range.end {
                    core::cmp::Ordering::Less
                } else {
                    core::cmp::Ordering::Equal
                }
            });

            match result {
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

impl NodePosition {
    fn new(position: FractionalIndex, idlp: IdLp) -> Self {
        Self { position, idlp }
    }
}

impl TreeState {
    pub fn new(idx: ContainerIdx, peer_id: PeerID) -> Self {
        Self {
            idx,
            trees: FxHashMap::default(),
            children: Default::default(),
            fractional_index_config: TreeFractionalIndexConfigInner::default(),
            peer_id,
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
        if let Some(old_parent) = self.trees.get(&target).map(|x| x.parent) {
            // remove old position
            self.delete_position(&old_parent, &target);
        }

        let entry = self.children.entry(parent).or_default();
        let node_position = NodePosition::new(position.clone().unwrap_or_default(), id.idlp());
        debug_assert!(!entry.has_child(&node_position));
        entry.insert_child(node_position, target);

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

    fn _init_push_tree_node_in_order(
        &mut self,
        target: TreeID,
        parent: TreeParentId,
        last_move_op: IdFull,
        position: Option<FractionalIndex>,
    ) -> Result<(), LoroError> {
        debug_assert!(!self.trees.contains_key(&target));
        let entry = self.children.entry(parent).or_default();
        let node_position =
            NodePosition::new(position.clone().unwrap_or_default(), last_move_op.idlp());
        debug_assert!(!entry.has_child(&node_position));
        entry.push_child_in_order(node_position, target);
        self.trees.insert(
            target,
            TreeStateNode {
                parent,
                position,
                last_move_op,
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

    /// Get the parent of the node, if the node does not exist, return None
    pub fn parent(&self, target: &TreeID) -> Option<TreeParentId> {
        self.trees.get(target).map(|x| x.parent)
    }

    /// If the node is unexist, return None. If the node is deleted, return true.
    pub(crate) fn is_node_deleted(&self, target: &TreeID) -> Option<bool> {
        match self.trees.get(target).map(|x| x.parent)? {
            TreeParentId::Deleted => Some(true),
            TreeParentId::Root => Some(false),
            TreeParentId::Node(p) => self.is_node_deleted(&p),
            TreeParentId::Unexist => unreachable!(),
        }
    }

    pub(crate) fn is_node_unexist(&self, target: &TreeID) -> bool {
        !self.trees.contains_key(target)
    }

    // Get all flat tree nodes, excluding deleted nodes.
    pub(crate) fn tree_nodes(&self) -> Vec<TreeNode> {
        self.get_all_tree_nodes_under(TreeParentId::Root)
    }

    // Get all flat deleted nodes
    #[allow(unused)]
    pub(crate) fn deleted_tree_nodes(&self) -> Vec<TreeNode> {
        self.get_all_tree_nodes_under(TreeParentId::Deleted)
    }

    // Get all flat tree nodes under the parent
    pub(crate) fn get_all_tree_nodes_under(&self, root: TreeParentId) -> Vec<TreeNode> {
        let mut ans = vec![];
        let children = self.children.get(&root);
        let mut q = children
            .map(|x| VecDeque::from_iter(x.iter().enumerate().zip(std::iter::repeat(root))))
            .unwrap_or_default();

        while let Some(((index, (position, &target)), parent)) = q.pop_front() {
            ans.push(TreeNode {
                id: target,
                parent,
                fractional_index: position.position.clone(),
                index,
                last_move_op: self.trees.get(&target).map(|x| x.last_move_op).unwrap(),
            });
            if let Some(children) = self.children.get(&TreeParentId::Node(target)) {
                q.extend(
                    children
                        .iter()
                        .enumerate()
                        .map(|(index, (position, this_target))| {
                            ((index, (position, this_target)), TreeParentId::Node(target))
                        }),
                );
            }
        }
        ans
    }

    pub(crate) fn get_all_hierarchy_nodes_under(
        &self,
        root: TreeParentId,
    ) -> Vec<TreeNodeWithChildren> {
        let mut ans = vec![];
        let Some(children) = self.children.get(&root) else {
            return ans;
        };
        for (index, (position, &target)) in children.iter().enumerate() {
            ans.push(TreeNodeWithChildren {
                id: target,
                parent: root,
                fractional_index: position.position.clone(),
                index,
                children: self.get_all_hierarchy_nodes_under(TreeParentId::Node(target)),
            })
        }
        ans
    }

    fn bfs_all_alive_nodes_for_fast_snapshot(&self) -> Vec<TreeNode> {
        let mut ans = vec![];
        self._bfs_all_nodes(TreeParentId::Root, &mut ans);
        ans
    }

    fn bfs_all_deleted_nodes_for_fast_snapshot(&self) -> Vec<TreeNode> {
        let mut ans = vec![];
        self._bfs_all_nodes(TreeParentId::Deleted, &mut ans);
        ans
    }

    fn _bfs_all_nodes(&self, root: TreeParentId, ans: &mut Vec<TreeNode>) {
        let children = self.children.get(&root);
        if let Some(children) = children {
            for (index, (position, target)) in children.iter().enumerate() {
                ans.push(TreeNode {
                    id: *target,
                    parent: root,
                    fractional_index: position.position.clone(),
                    index,
                    last_move_op: self.trees.get(target).map(|x| x.last_move_op).unwrap(),
                });
            }

            for (_, id) in children.iter() {
                self._bfs_all_nodes(TreeParentId::Node(*id), ans);
            }
        }
    }

    pub fn nodes(&self) -> Vec<TreeID> {
        self.trees.keys().copied().collect::<Vec<_>>()
    }

    #[cfg(feature = "test_utils")]
    pub fn max_counter(&self) -> i32 {
        self.trees
            .keys()
            .filter(|&k| !self.is_node_deleted(k).unwrap())
            .map(|k| k.counter)
            .max()
            .unwrap_or(0)
    }

    pub fn get_children<'a>(
        &'a self,
        parent: &TreeParentId,
    ) -> Option<impl Iterator<Item = TreeID> + 'a> {
        self.children.get(parent).map(|x| x.iter().map(|x| *x.1))
    }

    pub fn children_num(&self, parent: &TreeParentId) -> Option<usize> {
        self.children.get(parent).map(|x| x.len())
    }

    /// Determine whether the target is the child of the node
    ///
    /// O(1)
    pub fn is_parent(&self, target: &TreeID, parent: &TreeParentId) -> bool {
        self.trees
            .get(target)
            .map_or(false, |x| x.parent == *parent)
    }

    /// Delete the position cache of the node
    pub(crate) fn delete_position(&mut self, parent: &TreeParentId, target: &TreeID) {
        if let Some(x) = self.children.get_mut(parent) {
            x.delete_child(target);
        }
    }

    pub(crate) fn generate_position_at(
        &mut self,
        target: &TreeID,
        parent: &TreeParentId,
        index: usize,
    ) -> FractionalIndexGenResult {
        match &mut self.fractional_index_config {
            TreeFractionalIndexConfigInner::GenerateFractionalIndex { jitter, rng } => {
                if *jitter == 0 {
                    self.children
                        .entry(*parent)
                        .or_default()
                        .generate_fi_at(index, target)
                } else {
                    self.children
                        .entry(*parent)
                        .or_default()
                        .generate_fi_at_jitter(index, target, rng.as_mut(), *jitter)
                }
            }
            TreeFractionalIndexConfigInner::AlwaysDefault => {
                FractionalIndexGenResult::Ok(FractionalIndex::default())
            }
        }
    }

    pub(crate) fn is_fractional_index_enabled(&self) -> bool {
        !matches!(
            self.fractional_index_config,
            TreeFractionalIndexConfigInner::AlwaysDefault
        )
    }

    pub(crate) fn enable_generate_fractional_index(&mut self, jitter: u8) {
        if let TreeFractionalIndexConfigInner::GenerateFractionalIndex {
            jitter: old_jitter, ..
        } = &mut self.fractional_index_config
        {
            *old_jitter = jitter;
            return;
        }
        self.fractional_index_config = TreeFractionalIndexConfigInner::GenerateFractionalIndex {
            jitter,
            rng: Box::new(rand::rngs::StdRng::seed_from_u64(self.peer_id)),
        };
    }

    pub(crate) fn disable_generate_fractional_index(&mut self) {
        self.fractional_index_config = TreeFractionalIndexConfigInner::AlwaysDefault;
    }

    pub(crate) fn get_position(&self, target: &TreeID) -> Option<FractionalIndex> {
        self.trees.get(target).and_then(|x| x.position.clone())
    }

    pub(crate) fn get_index_by_tree_id(&self, target: &TreeID) -> Option<usize> {
        let parent = self.parent(target)?;
        (!parent.is_deleted())
            .then(|| {
                self.children
                    .get(&parent)
                    .and_then(|x| x.get_index_by_child_id(target))
            })
            .flatten()
    }

    pub(crate) fn get_index_by_position(
        &self,
        parent: &TreeParentId,
        node_position: &NodePosition,
    ) -> Option<usize> {
        self.children.get(parent).map(|c| {
            match c.get_last_insert_index_by_position(node_position) {
                Ok(i) => i,
                Err(i) => i,
            }
        })
    }

    pub(crate) fn get_id_by_index(&self, parent: &TreeParentId, index: usize) -> Option<TreeID> {
        (!parent.is_deleted())
            .then(|| self.children.get(parent).and_then(|x| x.get_id_at(index)))
            .flatten()
    }

    /// Check the consistency between `self.trees` and `self.children`
    ///
    /// It's used for debug and test
    #[allow(unused)]
    pub(crate) fn check_tree_integrity(&self) {
        let mut parent_children_map: FxHashMap<TreeParentId, FxHashSet<TreeID>> =
            FxHashMap::default();
        for (id, node) in self.trees.iter() {
            let parent = node.parent;
            parent_children_map.entry(parent).or_default().insert(*id);
        }

        for (parent, children) in parent_children_map.iter() {
            let cached_children = self.get_children(parent).expect("parent not found");
            let cached_children = cached_children.collect::<FxHashSet<_>>();
            if &cached_children != children {
                panic!(
                    "tree integrity broken: children set of node {:?} is not consistent",
                    parent
                );
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        match self.children.get(&TreeParentId::Root) {
            Some(c) => c.len() == 0,
            None => true,
        }
    }

    pub fn get_last_move_id(&self, id: &TreeID) -> Option<ID> {
        self.trees.get(id).map(|x| x.last_move_op.id())
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

    // How we apply the diff is coupled with the [DiffMode] we used to calculate the diff.
    // So be careful when you modify this function.
    fn apply_diff_and_convert(
        &mut self,
        diff: crate::event::InternalDiff,
        ctx: DiffApplyContext,
    ) -> Diff {
        let need_check = !matches!(ctx.mode, DiffMode::Checkout | DiffMode::Linear);
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

                        if let TreeParentId::Node(p) = parent {
                            // reuse diff cache, at this time,it's "move in deleted"
                            if self.is_node_deleted(p).unwrap() {
                                continue;
                            }
                        }

                        let index = self.get_index_by_tree_id(&target).unwrap();
                        ans.push(TreeDiffItem {
                            target,
                            action: TreeExternalDiff::Create {
                                parent: *parent,
                                index,
                                position: position.clone(),
                            },
                        });
                    }
                    TreeInternalDiff::Move { parent, position } => {
                        let old_parent = self.trees.get(&target).unwrap().parent;
                        // If this is some, the node is still alive at the moment
                        let old_index = self.get_index_by_tree_id(&target);
                        let was_alive = !self.is_node_deleted(&target).unwrap();
                        if need_check {
                            if self
                                .mov(target, *parent, last_move_op, Some(position.clone()), true)
                                .is_ok()
                            {
                                if self.is_node_deleted(&target).unwrap() {
                                    if was_alive {
                                        // delete event
                                        ans.push(TreeDiffItem {
                                            target,
                                            action: TreeExternalDiff::Delete {
                                                old_parent,
                                                old_index: old_index.unwrap(),
                                            },
                                        });
                                    }
                                    // Otherwise, it's a normal move inside deleted nodes, no event is needed
                                } else if was_alive {
                                    // normal move
                                    ans.push(TreeDiffItem {
                                        target,
                                        action: TreeExternalDiff::Move {
                                            parent: *parent,
                                            index: self.get_index_by_tree_id(&target).unwrap(),
                                            position: position.clone(),
                                            old_parent,
                                            old_index: old_index.unwrap(),
                                        },
                                    });
                                } else {
                                    // create event
                                    ans.push(TreeDiffItem {
                                        target,
                                        action: TreeExternalDiff::Create {
                                            parent: *parent,
                                            index: self.get_index_by_tree_id(&target).unwrap(),
                                            position: position.clone(),
                                        },
                                    });
                                }
                            }
                        } else {
                            self.mov(target, *parent, last_move_op, Some(position.clone()), false)
                                .unwrap();

                            if let TreeParentId::Node(p) = parent {
                                // reuse diff cache, at this time,it's "move in deleted"
                                if self.is_node_deleted(p).unwrap() {
                                    continue;
                                }
                            }

                            let index = self.get_index_by_tree_id(&target).unwrap();
                            match was_alive {
                                true => {
                                    ans.push(TreeDiffItem {
                                        target,
                                        action: TreeExternalDiff::Move {
                                            parent: *parent,
                                            index,
                                            position: position.clone(),
                                            old_parent,
                                            old_index: old_index.unwrap(),
                                        },
                                    });
                                }
                                false => {
                                    ans.push(TreeDiffItem {
                                        target,
                                        action: TreeExternalDiff::Create {
                                            parent: *parent,
                                            index,
                                            position: position.clone(),
                                        },
                                    });
                                }
                            }
                        };
                    }
                    TreeInternalDiff::Delete { parent, position } => {
                        let mut send_event = true;
                        if self.is_node_deleted(&target).unwrap() {
                            send_event = false;
                        }
                        if send_event {
                            ans.push(TreeDiffItem {
                                target,
                                action: TreeExternalDiff::Delete {
                                    old_parent: self.trees.get(&target).unwrap().parent,
                                    old_index: self.get_index_by_tree_id(&target).unwrap(),
                                },
                            });
                        }
                        self.mov(target, *parent, last_move_op, position.clone(), false)
                            .unwrap();
                    }
                    TreeInternalDiff::MoveInDelete { parent, position } => {
                        self.mov(target, *parent, last_move_op, position.clone(), false)
                            .unwrap();
                    }
                    TreeInternalDiff::UnCreate => {
                        // maybe the node created and moved to the parent deleted
                        if !self.is_node_deleted(&target).unwrap() {
                            ans.push(TreeDiffItem {
                                target,
                                action: TreeExternalDiff::Delete {
                                    old_parent: self.trees.get(&target).unwrap().parent,
                                    old_index: self.get_index_by_tree_id(&target).unwrap(),
                                },
                            });
                        }
                        // delete it from state
                        let node = self.trees.remove(&target);
                        if let Some(node) = node {
                            if !node.parent.is_deleted() {
                                self.children
                                    .get_mut(&node.parent)
                                    .unwrap()
                                    .delete_child(&target);
                            }
                        }
                        continue;
                    }
                };
            }
        }

        // self.check_tree_integrity();
        Diff::Tree(TreeDiff { diff: ans })
    }

    // How we apply the diff is coupled with the [DiffMode] we used to calculate the diff.
    // So be careful when you modify this function.
    fn apply_diff(&mut self, diff: InternalDiff, ctx: DiffApplyContext) {
        if let InternalDiff::Tree(tree) = &diff {
            let need_check = !matches!(ctx.mode, DiffMode::Checkout | DiffMode::Linear);
            // assert never cause cycle move
            for diff in tree.diff.iter() {
                let last_move_op = diff.last_effective_move_op_id;
                let target = diff.target;
                // create associated metadata container
                match &diff.action {
                    TreeInternalDiff::Create { parent, position }
                    | TreeInternalDiff::Move {
                        parent, position, ..
                    } => {
                        if need_check {
                            self.mov(target, *parent, last_move_op, Some(position.clone()), true)
                                .unwrap_or_default();
                        } else {
                            self.mov(target, *parent, last_move_op, Some(position.clone()), false)
                                .unwrap();
                        }
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
        // self.check_tree_integrity();
    }

    fn apply_local_op(&mut self, raw_op: &RawOp, _op: &Op) -> LoroResult<ApplyLocalOpReturn> {
        match &raw_op.content {
            crate::op::RawOpContent::Tree(tree) => match &**tree {
                TreeOp::Create {
                    target,
                    parent,
                    position,
                }
                | TreeOp::Move {
                    target,
                    parent,
                    position,
                } => {
                    let parent = TreeParentId::from(*parent);
                    self.mov(
                        *target,
                        parent,
                        raw_op.id_full(),
                        Some(position.clone()),
                        true,
                    )?;
                }
                TreeOp::Delete { target } => {
                    let parent = TreeParentId::Deleted;
                    self.mov(*target, parent, raw_op.id_full(), None, true)?;
                }
            },
            _ => unreachable!(),
        }
        // self.check_tree_integrity();
        Ok(Default::default())
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
                    parent: node_parent,
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
        self.get_all_hierarchy_nodes_under(TreeParentId::Root)
            .into_iter()
            .map(|x| x.into_value())
            .collect::<Vec<_>>()
            .into()
    }

    /// Get the index of the child container
    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        let id = id.as_normal().unwrap();
        let tree_id = TreeID {
            peer: *id.0,
            counter: *id.1,
        };
        if !self.trees.contains_key(&tree_id) || self.is_node_deleted(&tree_id).unwrap() {
            None
        } else {
            Some(Index::Node(tree_id))
        }
    }

    fn contains_child(&self, id: &ContainerID) -> bool {
        let id = id.as_normal().unwrap();
        let tree_id = TreeID {
            peer: *id.0,
            counter: *id.1,
        };
        self.trees.contains_key(&tree_id) && !self.is_node_deleted(&tree_id).unwrap()
    }

    fn get_child_containers(&self) -> Vec<ContainerID> {
        self.trees
            .keys()
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
    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext) -> LoroResult<()> {
        assert_eq!(ctx.mode, EncodeMode::OutdatedSnapshot);
        for op in ctx.ops {
            assert_eq!(op.op.atom_len(), 1);
            let content = op.op.content.as_tree().unwrap();
            match &**content {
                TreeOp::Create {
                    target,
                    parent,
                    position,
                }
                | TreeOp::Move {
                    target,
                    parent,
                    position,
                } => {
                    let parent = TreeParentId::from(*parent);
                    self.mov(*target, parent, op.id_full(), Some(position.clone()), false)
                        .unwrap()
                }
                TreeOp::Delete { target } => {
                    let parent = TreeParentId::Deleted;
                    self.mov(*target, parent, op.id_full(), None, false)
                        .unwrap()
                }
            };
        }
        Ok(())
    }

    fn fork(&self, _config: &Configure) -> Self {
        self.clone()
    }
}

// convert map container to LoroValue
#[allow(clippy::ptr_arg)]
pub(crate) fn get_meta_value(nodes: &mut Vec<LoroValue>, state: &mut DocState) {
    for node in nodes.iter_mut() {
        let map = node.as_map_mut().unwrap().make_mut();
        let meta = map.get_mut("meta").unwrap();
        let id = meta.as_container().unwrap();
        *meta = state.get_container_deep_value(state.arena.register_container(id));
        let children = map.get_mut("children").unwrap().as_list_mut().unwrap();
        get_meta_value(children.make_mut(), state);
    }
}

#[derive(Debug, Clone)]
pub struct TreeNode {
    pub id: TreeID,
    pub parent: TreeParentId,
    pub fractional_index: FractionalIndex,
    pub index: usize,
    pub last_move_op: IdFull,
}

#[derive(Debug, Clone)]
pub struct TreeNodeWithChildren {
    pub id: TreeID,
    pub parent: TreeParentId,
    pub fractional_index: FractionalIndex,
    pub index: usize,
    pub children: Vec<TreeNodeWithChildren>,
}

impl TreeNodeWithChildren {
    fn into_value(self) -> LoroValue {
        let mut t = FxHashMap::default();
        t.insert("id".to_string(), self.id.to_string().into());
        let p = self
            .parent
            .tree_id()
            .map(|p| p.to_string().into())
            .unwrap_or(LoroValue::Null);
        t.insert("parent".to_string(), p);
        t.insert(
            "meta".to_string(),
            self.id.associated_meta_container().into(),
        );
        t.insert("index".to_string(), (self.index as i64).into());
        t.insert(
            "fractional_index".to_string(),
            self.fractional_index.to_string().into(),
        );
        t.insert(
            "children".to_string(),
            self.children
                .into_iter()
                .map(|x| x.into_value())
                .collect::<Vec<_>>()
                .into(),
        );
        t.into()
    }
}

mod jitter {
    use super::{FractionalIndexGenResult, NodeChildren};
    use fractional_index::FractionalIndex;
    use loro_common::TreeID;
    use rand::Rng;

    impl NodeChildren {
        pub(super) fn generate_fi_at_jitter(
            &self,
            pos: usize,
            target: &TreeID,
            rng: &mut impl Rng,
            jitter: u8,
        ) -> FractionalIndexGenResult {
            let mut reset_ids = vec![];
            let mut left = None;
            let mut next_right = None;
            {
                let mut right = None;
                let children_num = self.len();
                if children_num == 0 {
                    return FractionalIndexGenResult::Ok(FractionalIndex::jitter_default(
                        rng, jitter,
                    ));
                }

                if pos > 0 {
                    left = self.get_node_position_at(pos - 1);
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
                            let this_position = &self.get_node_position_at(i).unwrap().position;
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
                        FractionalIndex::new_jitter(
                            left.map(|x| &x.position),
                            right.map(|x| &x.0.position),
                            rng,
                            jitter,
                        )
                        .unwrap(),
                    );
                }
            }
            let positions = FractionalIndex::generate_n_evenly_jitter(
                left.map(|x| &x.position),
                next_right.as_ref(),
                reset_ids.len() + 1,
                rng,
                jitter,
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

mod snapshot {
    use std::{borrow::Cow, collections::BTreeSet, io::Read};

    use fractional_index::FractionalIndex;
    use fxhash::FxHashMap;
    use itertools::Itertools;
    use loro_common::{IdFull, Lamport, PeerID, TreeID};

    use serde_columnar::columnar;

    use crate::{
        encoding::{arena::PositionArena, value_register::ValueRegister},
        state::FastStateSnapshot,
    };

    use super::{TreeNode, TreeParentId, TreeState};
    #[columnar(vec, ser, de, iterable)]
    #[derive(Debug, Clone)]
    struct EncodedTreeNodeId {
        #[columnar(strategy = "DeltaRle")]
        peer_idx: usize,
        #[columnar(strategy = "DeltaRle")]
        counter: i32,
    }

    #[columnar(vec, ser, de, iterable)]
    #[derive(Debug, Clone)]
    struct EncodedTreeNode {
        /// If this field is 0, it means none, its parent is root
        /// If this field is 1, its parent is the deleted root
        #[columnar(strategy = "DeltaRle")]
        parent_idx_plus_two: usize,
        #[columnar(strategy = "DeltaRle")]
        last_set_peer_idx: usize,
        #[columnar(strategy = "DeltaRle")]
        last_set_counter: i32,
        #[columnar(strategy = "DeltaRle")]
        last_set_lamport_sub_counter: i32,
        fractional_index_idx: usize,
    }

    #[columnar(ser, de)]
    struct EncodedTree<'a> {
        #[columnar(class = "vec", iter = "EncodedTreeNodeId")]
        node_ids: Vec<EncodedTreeNodeId>,
        #[columnar(class = "vec", iter = "EncodedTreeNode")]
        nodes: Vec<EncodedTreeNode>,
        #[columnar(borrow)]
        fractional_indexes: Cow<'a, [u8]>,
        #[columnar(borrow)]
        reserved_has_effect_bool_rle: Cow<'a, [u8]>,
    }

    fn encode(
        state: &TreeState,
        input: Vec<TreeNode>,
        deleted_nodes: Vec<TreeNode>,
    ) -> (ValueRegister<PeerID>, EncodedTree) {
        let mut peers: ValueRegister<PeerID> = ValueRegister::new();
        let mut position_set = BTreeSet::default();
        let mut nodes = Vec::with_capacity(input.len());
        let mut node_ids = Vec::with_capacity(input.len());
        let mut position_register = ValueRegister::new();
        let mut id_to_idx = FxHashMap::default();
        for node in input.iter().chain(deleted_nodes.iter()) {
            position_set.insert(node.fractional_index.clone());
            let idx = node_ids.len();
            node_ids.push(EncodedTreeNodeId {
                peer_idx: peers.register(&node.id.peer),
                counter: node.id.counter,
            });
            id_to_idx.insert(node.id, idx);
        }

        for p in position_set {
            position_register.register(&p);
        }

        for node in input {
            let n = state.trees.get(&node.id).unwrap();
            let last_set_id = n.last_move_op;
            nodes.push(EncodedTreeNode {
                parent_idx_plus_two: match node.parent {
                    TreeParentId::Deleted => 1,
                    TreeParentId::Root => 0,
                    TreeParentId::Node(id) => id_to_idx.get(&id).unwrap() + 2,
                    TreeParentId::Unexist => unreachable!(),
                },
                last_set_peer_idx: peers.register(&last_set_id.peer),
                last_set_counter: last_set_id.counter,
                last_set_lamport_sub_counter: last_set_id.lamport as i32 - last_set_id.counter,
                fractional_index_idx: position_register.register(&node.fractional_index),
            })
        }

        for node in deleted_nodes {
            let n = state.trees.get(&node.id).unwrap();
            let last_set_id = n.last_move_op;
            nodes.push(EncodedTreeNode {
                parent_idx_plus_two: match node.parent {
                    TreeParentId::Deleted => 1,
                    TreeParentId::Root => 0,
                    TreeParentId::Node(id) => id_to_idx.get(&id).unwrap() + 2,
                    TreeParentId::Unexist => unreachable!(),
                },
                last_set_peer_idx: peers.register(&last_set_id.peer),
                last_set_counter: last_set_id.counter,
                last_set_lamport_sub_counter: last_set_id.lamport as i32 - last_set_id.counter,
                fractional_index_idx: position_register.register(&node.fractional_index),
            })
        }

        let position_vec = position_register.unwrap_vec();
        let positions = PositionArena::from_positions(position_vec.iter().map(|p| p.as_bytes()));
        (
            peers,
            EncodedTree {
                node_ids,
                nodes,
                fractional_indexes: positions.encode().into(),
                reserved_has_effect_bool_rle: vec![].into(),
            },
        )
    }

    impl FastStateSnapshot for TreeState {
        /// Encodes the TreeState into a compact binary format for efficient serialization.
        ///
        /// The encoding schema:
        /// 1. Encodes all nodes using a breadth-first search traversal, ensuring a consistent order.
        /// 2. Uses a ValueRegister to deduplicate and index PeerIDs and TreeIDs, reducing redundancy.
        ///    - PeerIDs are stored once and referenced by index.
        ///    - TreeIDs are decomposed into peer index and counter for compact representation.
        /// 3. Encodes fractional indexes using a PositionArena for space efficiency
        /// 4. Utilizes delta encoding and run-length encoding for further size reduction:
        ///    - Delta encoding stores differences between consecutive values.
        ///    - Run-length encoding compresses sequences of repeated values.
        /// 5. Stores parent relationships using indices, with special values for root and deleted nodes.
        /// 6. Encodes last move operation details (peer_idx, counter[Delta], lamport clock[Delta]) for each node.
        fn encode_snapshot_fast<W: std::io::prelude::Write>(&mut self, mut w: W) {
            let all_alive_nodes = self.bfs_all_alive_nodes_for_fast_snapshot();
            let all_deleted_nodes = self.bfs_all_deleted_nodes_for_fast_snapshot();
            let (peers, encoded) = encode(self, all_alive_nodes, all_deleted_nodes);
            let peers = peers.unwrap_vec();
            leb128::write::unsigned(&mut w, peers.len() as u64).unwrap();
            for peer in peers {
                w.write_all(&peer.to_le_bytes()).unwrap();
            }

            let vec = serde_columnar::to_vec(&encoded).unwrap();
            w.write_all(&vec).unwrap();
        }

        fn decode_value(_: &[u8]) -> loro_common::LoroResult<(loro_common::LoroValue, &[u8])> {
            unreachable!()
        }

        fn decode_snapshot_fast(
            idx: crate::container::idx::ContainerIdx,
            (_, mut bytes): (loro_common::LoroValue, &[u8]),
            ctx: crate::state::ContainerCreationContext,
        ) -> loro_common::LoroResult<Self>
        where
            Self: Sized,
        {
            let peer_num = leb128::read::unsigned(&mut bytes).unwrap() as usize;
            let mut peers = Vec::with_capacity(peer_num);
            for _ in 0..peer_num {
                let mut buf = [0u8; 8];
                bytes.read_exact(&mut buf).unwrap();
                peers.push(PeerID::from_le_bytes(buf));
            }

            let mut tree = TreeState::new(idx, ctx.peer);
            let encoded: EncodedTree = serde_columnar::from_bytes(bytes)?;
            let fractional_indexes = PositionArena::decode(&encoded.fractional_indexes).unwrap();
            let fractional_indexes = fractional_indexes.parse_to_positions();
            let node_ids = encoded
                .node_ids
                .iter()
                .map(|x| TreeID::new(peers[x.peer_idx], x.counter))
                .collect_vec();
            for (node_id, node) in node_ids.iter().zip(encoded.nodes.into_iter()) {
                // PERF: we don't need to mov the deleted node, instead we can cache them
                // If the parent is TreeParentId::Deleted, then all the nodes afterwards are deleted
                tree._init_push_tree_node_in_order(
                    *node_id,
                    match node.parent_idx_plus_two {
                        0 => TreeParentId::Root,
                        1 => TreeParentId::Deleted,
                        n => {
                            let id = node_ids[n - 2];
                            TreeParentId::from(Some(id))
                        }
                    },
                    IdFull::new(
                        peers[node.last_set_peer_idx],
                        node.last_set_counter,
                        (node.last_set_lamport_sub_counter + node.last_set_counter) as Lamport,
                    ),
                    Some(FractionalIndex::from_bytes(
                        fractional_indexes[node.fractional_index_idx].clone(),
                    )),
                )
                .unwrap();
            }

            Ok(tree)
        }
    }

    #[cfg(test)]
    mod test {
        use loro_common::LoroValue;

        use crate::{
            container::idx::ContainerIdx,
            state::{ContainerCreationContext, ContainerState},
            LoroDoc,
        };

        use super::*;

        #[test]
        fn test_tree_state_snapshot() {
            let doc = LoroDoc::new();
            doc.set_peer_id(0).unwrap();
            doc.start_auto_commit();
            let tree = doc.get_tree("tree");
            tree.enable_fractional_index(0);
            let a = tree.create(TreeParentId::Root).unwrap();
            let b = tree.create(TreeParentId::Root).unwrap();
            let _c = tree.create(TreeParentId::Root).unwrap();
            tree.mov(b, TreeParentId::Node(a)).unwrap();
            let (bytes, value) = {
                let mut doc_state = doc.app_state().try_lock().unwrap();
                let tree_state = doc_state.get_tree("tree").unwrap();
                let value = tree_state.get_value();
                let mut bytes = Vec::new();
                tree_state.encode_snapshot_fast(&mut bytes);
                (bytes, value)
            };

            assert!(bytes.len() == 55, "{}", bytes.len());
            let mut new_tree_state = TreeState::decode_snapshot_fast(
                ContainerIdx::from_index_and_type(0, loro_common::ContainerType::Tree),
                (LoroValue::Null, &bytes),
                ContainerCreationContext {
                    configure: &Default::default(),
                    peer: 0,
                },
            )
            .unwrap();

            let mut doc_state = doc.app_state().try_lock().unwrap();
            let tree_state = doc_state.get_tree("tree").unwrap();
            assert_eq!(&tree_state.trees, &new_tree_state.trees);
            let new_v = new_tree_state.get_value();
            assert_eq!(value, new_v);
        }
    }
}
