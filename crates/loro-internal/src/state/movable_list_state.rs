use itertools::Itertools;
use loro_delta::{array_vec::ArrayVec, DeltaRope, DeltaRopeBuilder};
use serde_columnar::columnar;
use std::sync::{Arc, Mutex, Weak};
use tracing::{debug, instrument, trace_span, warn};

use fxhash::FxHashMap;
use generic_btree::BTree;
use loro_common::{CompactIdLp, ContainerID, IdFull, IdLp, LoroResult, LoroValue, ID};

use crate::{
    arena::SharedArena,
    container::{idx::ContainerIdx, list::list_op::ListOp},
    delta::DeltaItem,
    encoding::{StateSnapshotDecodeContext, StateSnapshotEncoder},
    event::{Diff, Index, InternalDiff, ListDeltaMeta},
    handler::ValueOrHandler,
    op::{ListSlice, Op, RawOp},
    state::movable_list_state::inner::PushElemInfo,
    txn::Transaction,
    ApplyDiff, DocState, ListDiff,
};

use self::{
    inner::{InnerState, UpdateResultFromPosChange},
    list_item_tree::{MovableListTreeTrait, OpLenQuery, UserLenQuery},
};

use super::ContainerState;

#[derive(Debug, Clone)]
pub struct MovableListState {
    idx: ContainerIdx,
    inner: InnerState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    pointed_by: Option<CompactIdLp>,
    id: IdFull,
}

#[derive(Debug, Clone)]
pub(crate) struct Element {
    value: LoroValue,
    value_id: IdLp,
    pos: IdLp,
}

impl Element {
    pub(crate) fn value(&self) -> &LoroValue {
        &self.value
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexType {
    /// This includes the ones that are not being pointed at, which may not be visible to users
    ForOp,
    /// This only includes the ones that are being pointed at, which means visible to users
    ForUser,
}

mod list_item_tree {
    use std::{
        iter::Sum,
        ops::{Add, AddAssign, Sub},
    };

    use generic_btree::{
        rle::{CanRemove, HasLength, Mergeable, Sliceable, TryInsert},
        BTreeTrait,
    };

    use crate::utils::query_by_len::{IndexQuery, QueryByLen};

    use super::ListItem;

    impl HasLength for ListItem {
        fn rle_len(&self) -> usize {
            1
        }
    }

    impl Mergeable for ListItem {
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

    impl Sliceable for ListItem {
        fn _slice(&self, range: std::ops::Range<usize>) -> Self {
            assert_eq!(range.len(), 1);
            self.clone()
        }
    }

    impl CanRemove for ListItem {
        fn can_remove(&self) -> bool {
            false
        }
    }

    impl TryInsert for ListItem {
        fn try_insert(&mut self, pos: usize, elem: Self) -> Result<(), Self>
        where
            Self: Sized,
        {
            Err(elem)
        }
    }

    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    pub(super) struct Cache {
        /// This length info include the ones that were deleted.
        /// It's used in op and diff calculation.
        pub include_dead_len: i32,
        /// This length info does not include the ones that were deleted.
        /// So it's facing the users.
        pub user_len: i32,
    }

    impl Add for Cache {
        type Output = Self;

        fn add(self, rhs: Self) -> Self::Output {
            Self {
                include_dead_len: self.include_dead_len + rhs.include_dead_len,
                user_len: self.user_len + rhs.user_len,
            }
        }
    }

    impl AddAssign for Cache {
        fn add_assign(&mut self, rhs: Self) {
            *self = *self + rhs;
        }
    }

    impl Sub for Cache {
        type Output = Self;

        fn sub(self, rhs: Self) -> Self::Output {
            Self {
                include_dead_len: self.include_dead_len - rhs.include_dead_len,
                user_len: self.user_len - rhs.user_len,
            }
        }
    }

    impl Sum for Cache {
        fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
            iter.fold(Self::default(), |acc, x| acc + x)
        }
    }

    impl CanRemove for Cache {
        fn can_remove(&self) -> bool {
            self.include_dead_len == 0
        }
    }

    pub(super) struct MovableListTreeTrait;

    impl BTreeTrait for MovableListTreeTrait {
        type Elem = ListItem;

        type Cache = Cache;

        type CacheDiff = Cache;

        fn calc_cache_internal(
            cache: &mut Self::Cache,
            caches: &[generic_btree::Child<Self>],
        ) -> Self::CacheDiff {
            let new: Cache = caches.iter().map(|x| *x.cache()).sum();
            let diff = new - *cache;
            *cache = new;
            diff
        }

        fn apply_cache_diff(cache: &mut Self::Cache, diff: &Self::CacheDiff) {
            *cache += *diff;
        }

        fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
            *diff1 += *diff2;
        }

        fn get_elem_cache(elem: &Self::Elem) -> Self::Cache {
            if elem.pointed_by.is_some() {
                Cache {
                    include_dead_len: 1,
                    user_len: 1,
                }
            } else {
                Cache {
                    include_dead_len: 1,
                    user_len: 0,
                }
            }
        }

        fn new_cache_to_diff(cache: &Self::Cache) -> Self::CacheDiff {
            *cache
        }

        fn sub_cache(cache_lhs: &Self::Cache, cache_rhs: &Self::Cache) -> Self::CacheDiff {
            *cache_lhs - *cache_rhs
        }
    }

    pub(crate) struct UserLenQueryT;
    pub(crate) type UserLenQuery = IndexQuery<UserLenQueryT, MovableListTreeTrait>;

    impl QueryByLen<MovableListTreeTrait> for UserLenQueryT {
        fn get_cache_len(cache: &<MovableListTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.user_len as usize
        }

        fn get_elem_len(elem: &<MovableListTreeTrait as BTreeTrait>::Elem) -> usize {
            if elem.pointed_by.is_some() {
                1
            } else {
                0
            }
        }

        fn get_offset_and_found(
            left: usize,
            elem: &<MovableListTreeTrait as BTreeTrait>::Elem,
        ) -> (usize, bool) {
            if left == 0 && elem.pointed_by.is_some() {
                (0, true)
            } else {
                (1, false)
            }
        }

        fn get_cache_entity_len(cache: &<MovableListTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.user_len as usize
        }
    }

    pub(crate) struct IncludeDeadLenQueryT;
    pub(crate) type OpLenQuery = IndexQuery<IncludeDeadLenQueryT, MovableListTreeTrait>;

    impl QueryByLen<MovableListTreeTrait> for IncludeDeadLenQueryT {
        fn get_cache_len(cache: &<MovableListTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.include_dead_len as usize
        }

        fn get_elem_len(_elem: &<MovableListTreeTrait as BTreeTrait>::Elem) -> usize {
            1
        }

        fn get_offset_and_found(
            left: usize,
            _elem: &<MovableListTreeTrait as BTreeTrait>::Elem,
        ) -> (usize, bool) {
            if left == 0 {
                return (0, true);
            }

            (1, false)
        }

        fn get_cache_entity_len(cache: &<MovableListTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.include_dead_len as usize
        }
    }
}

/// The inner state of the list.
///
/// The operations inside this mod need to ensure certain invariants.
///
/// - list items' `pointed_by` must be consistent with the `elements`' `pos`.
/// - `id_to_list_leaf` must be consistent with the list.
/// - `child_container_to_elem` must be consistent with the element.
mod inner {
    use fxhash::{FxHashMap, FxHashSet};
    use generic_btree::{BTree, Cursor, LeafIndex, Query};
    use loro_common::{CompactIdLp, ContainerID, IdFull, IdLp, LoroValue, PeerID};
    use tracing::error;

    use super::{
        list_item_tree::{MovableListTreeTrait, OpLenQuery, UserLenQuery},
        Element, IndexType, ListItem,
    };

    #[derive(Debug, Clone)]
    pub(super) struct InnerState {
        list: BTree<MovableListTreeTrait>,
        id_to_list_leaf: FxHashMap<IdLp, LeafIndex>,
        elements: FxHashMap<CompactIdLp, Element>,
        /// This mapping may be out of date when elem is removed/updated.
        /// But it's guaranteed that if there is a ContainerID in the actual list,
        /// it will be mapped correctly.
        child_container_to_elem: FxHashMap<ContainerID, CompactIdLp>,
    }

    fn eq<T: PartialEq>(a: T, b: T) -> Result<(), ()> {
        if a == b {
            Ok(())
        } else {
            Err(())
        }
    }

    impl InnerState {
        pub fn new() -> Self {
            Self {
                list: BTree::new(),
                id_to_list_leaf: FxHashMap::default(),
                elements: FxHashMap::default(),
                child_container_to_elem: FxHashMap::default(),
            }
        }

        #[inline]
        pub fn child_container_to_elem(&self) -> &FxHashMap<ContainerID, CompactIdLp> {
            &self.child_container_to_elem
        }

        #[inline]
        pub fn elements(&self) -> &FxHashMap<CompactIdLp, Element> {
            &self.elements
        }

        #[inline]
        pub fn list(&self) -> &BTree<MovableListTreeTrait> {
            &self.list
        }

        pub fn remove_elem_by_id(&mut self, elem_id: &CompactIdLp) {
            self.elements.remove(elem_id);
        }

        #[allow(dead_code)]
        pub fn check_consistency(&self) {
            let mut failed = false;
            if self.check_list_item_consistency().is_err() {
                error!("list item consistency check failed, self={:#?}", self);
                failed = true;
            }

            if self.check_child_container_to_elem_consistency().is_err() {
                error!(
                    "child container to elem consistency check failed, self={:#?}",
                    self
                );
                failed = true;
            }

            if failed {
                panic!("consistency check failed");
            }
        }

        fn check_list_item_consistency(&self) -> Result<(), ()> {
            let mut visited_ids = FxHashSet::default();
            for list_item in self.list.iter() {
                {
                    if visited_ids.contains(&list_item.id.idlp()) {
                        error!("duplicate list item id");
                        return Err(());
                    }
                    visited_ids.insert(list_item.id.idlp());
                }
                let leaf = self.id_to_list_leaf.get(&list_item.id.idlp()).unwrap();
                let list_item_found = self.list.get_elem(*leaf).unwrap();
                eq(list_item_found, list_item)?;
                if let Some(pointed_by) = list_item_found.pointed_by {
                    let elem = self.elements.get(&pointed_by).unwrap();
                    eq(elem.pos, list_item.id.idlp())?;
                }
            }

            for (elem_id, elem) in self.elements.iter() {
                if let Some(item) = self.get_list_item_by_id(elem.pos) {
                    eq(item.pointed_by, Some(*elem_id))?;
                }
            }

            Ok(())
        }

        fn check_child_container_to_elem_consistency(&self) -> Result<(), ()> {
            for (elem_id, elem) in self.elements.iter() {
                if let LoroValue::Container(c) = &elem.value {
                    eq(self.child_container_to_elem.get(c), Some(elem_id))?;
                }
            }

            Ok(())
        }

        pub fn get_list_item_index(&self, id: IdLp, kind: IndexType) -> Option<usize> {
            self.id_to_list_leaf
                .get(&id)
                .map(|leaf| self.get_index_of(*leaf, kind) as usize)
        }

        pub fn get_index_of(&self, leaf: LeafIndex, kind: IndexType) -> i32 {
            let mut ans = 0;
            self.list
                .visit_previous_caches(Cursor { leaf, offset: 0 }, |cache| match cache {
                    generic_btree::PreviousCache::NodeCache(c) => {
                        if matches!(kind, IndexType::ForUser) {
                            ans += c.user_len;
                        } else {
                            ans += c.include_dead_len;
                        }
                    }
                    generic_btree::PreviousCache::PrevSiblingElem(p) => {
                        if matches!(kind, IndexType::ForUser) {
                            ans += if p.pointed_by.is_some() { 1 } else { 0 };
                        } else {
                            ans += 1;
                        }
                    }
                    generic_btree::PreviousCache::ThisElemAndOffset { .. } => {}
                });
            ans
        }

        #[inline]
        pub fn get_list_item_at(&self, pos: usize, index_type: IndexType) -> Option<&ListItem> {
            let index = match index_type {
                IndexType::ForUser => self.list.query::<UserLenQuery>(&pos).unwrap(),
                IndexType::ForOp => self.list.query::<OpLenQuery>(&pos).unwrap(),
            };
            self.list.get_elem(index.leaf())
        }

        pub fn get_child_index(&self, id: &ContainerID, index_type: IndexType) -> Option<usize> {
            let ans = self.child_container_to_elem.get(id).and_then(|eid| {
                let this = &self;
                let elem_id = eid.to_id().compact();
                let elem = this.elements.get(&elem_id)?;
                if elem.value.as_container() != Some(id) {
                    // TODO: may be better to find a way clean up these invalid elements?
                    return None;
                }
                if let Some(leaf) = self.id_to_list_leaf.get(&elem.pos) {
                    let list_item = self.list.get_elem(*leaf)?;
                    assert_eq!(list_item.pointed_by, Some(elem_id));
                } else {
                    return None;
                }
                this.get_list_item_index(elem.pos, index_type)
            });

            ans
        }

        #[inline]
        pub fn get_list_item_by_id(&self, id: IdLp) -> Option<&ListItem> {
            self.id_to_list_leaf
                .get(&id)
                .map(|&leaf| self.list.get_elem(leaf).unwrap())
        }

        pub fn len_kind(&self, kind: IndexType) -> usize {
            match kind {
                IndexType::ForUser => self.list.root_cache().user_len as usize,
                IndexType::ForOp => self.list.root_cache().include_dead_len as usize,
            }
        }

        /// Insert a new list item at the given position (op index).
        /// Return the value if the insertion activated an element.
        pub fn insert_list_item(&mut self, pos: usize, list_item_id: IdFull) {
            let c = self
                .list
                .insert::<OpLenQuery>(
                    &pos,
                    ListItem {
                        pointed_by: None,
                        id: list_item_id,
                    },
                )
                .0;
            self.id_to_list_leaf.insert(list_item_id.idlp(), c.leaf);
        }

        /// Drain the list items in the given range (op index).
        pub fn list_drain(
            &mut self,
            range: std::ops::Range<usize>,
            mut on_elem_id: impl FnMut(CompactIdLp, &Element),
        ) {
            for item in Self::drain_by_query::<OpLenQuery>(&mut self.list, range) {
                self.id_to_list_leaf.remove(&item.id.idlp());
                if let Some(elem_id) = &item.pointed_by {
                    let elem = self.elements.get(elem_id).unwrap();
                    on_elem_id(*elem_id, elem);
                    self.elements.remove(elem_id);
                }
            }
        }

        fn drain_by_query<Q: Query<MovableListTreeTrait>>(
            list: &mut BTree<MovableListTreeTrait>,
            range: std::ops::Range<Q::QueryArg>,
        ) -> generic_btree::iter::Drain<'_, MovableListTreeTrait> {
            let start = list.query::<Q>(&range.start);
            let end = list.query::<Q>(&range.end);
            generic_btree::iter::Drain::new(list, start, end)
        }

        /// Update the pos pointer of an element and the corresponding list items.
        ///
        /// The old list item will be removed if `remove_old` is true; Otherwise,
        /// it will be updated with its `pointed_by` removed.
        ///
        #[must_use]
        pub fn update_pos(
            &mut self,
            elem_id: CompactIdLp,
            new_pos: IdLp,
            remove_old: bool,
        ) -> UpdateResultFromPosChange {
            let mut old_item_id = None;
            if let Some(element) = self.elements.get_mut(&elem_id) {
                if element.pos != new_pos {
                    old_item_id = Some(element.pos);
                    element.pos = new_pos;
                } else {
                    return UpdateResultFromPosChange {
                        activate_new_list_item: false,
                        new_list_item_leaf: None,
                        removed_old_list_item_leaf: None,
                    };
                }
            } else {
                self.elements.insert(
                    elem_id,
                    Element {
                        value: LoroValue::Null,
                        value_id: IdLp::NONE_ID,
                        pos: new_pos,
                    },
                );
            }

            let mut ans = UpdateResultFromPosChange {
                activate_new_list_item: true,
                new_list_item_leaf: None,
                removed_old_list_item_leaf: None,
            };
            if let Some(leaf) = self.id_to_list_leaf.get(&new_pos) {
                ans.new_list_item_leaf = Some(*leaf);
                self.list.update_leaf(*leaf, |elem| {
                    ans.activate_new_list_item = elem.pointed_by.is_none();
                    debug_assert!(ans.activate_new_list_item);
                    elem.pointed_by = Some(elem_id);
                    (true, None, None)
                });
            } else {
                ans.activate_new_list_item = false;
            }

            if let Some(old) = old_item_id {
                if old.is_none() || old == new_pos {
                    return ans;
                }

                if remove_old {
                    let leaf = self.id_to_list_leaf.remove(&old).unwrap();
                    let elem = self.list.remove_leaf(Cursor { leaf, offset: 0 }).unwrap();
                    assert_eq!(elem.pointed_by, Some(elem_id));
                } else if let Some(leaf) = self.id_to_list_leaf.get(&old) {
                    ans.removed_old_list_item_leaf = Some(*leaf);
                    let (still_valid, split) = self.list.update_leaf(*leaf, |item| {
                        item.pointed_by = None;
                        (true, None, None)
                    });
                    assert!(still_valid);
                    assert!(split.arr.is_empty());
                }
            }

            ans
        }

        /// The update can reflected on an event exposed to users. This method calculates the
        /// corresponding event position information.
        pub fn convert_update_to_event_pos(
            &self,
            update: UpdateResultFromPosChange,
        ) -> EventPosInfo {
            if update.activate_new_list_item {
                let new = update.new_list_item_leaf.unwrap();
                if let Some(del) = update.removed_old_list_item_leaf {
                    let mut insert_pos = self.get_index_of(new, IndexType::ForUser) as usize;
                    let del_pos = self.get_index_of(del, IndexType::ForUser) as usize;
                    if insert_pos > del_pos {
                        insert_pos += 1;
                    }
                    EventPosInfo {
                        activate_new_list_item: update.activate_new_list_item,
                        insert: Some(insert_pos),
                        delete: Some(del_pos),
                    }
                } else {
                    EventPosInfo {
                        activate_new_list_item: update.activate_new_list_item,
                        insert: Some(self.get_index_of(new, IndexType::ForUser) as usize),
                        delete: None,
                    }
                }
            } else if let Some(del) = update.removed_old_list_item_leaf {
                EventPosInfo {
                    activate_new_list_item: update.activate_new_list_item,
                    insert: None,
                    delete: Some(self.get_index_of(del, IndexType::ForUser) as usize),
                }
            } else {
                EventPosInfo {
                    activate_new_list_item: update.activate_new_list_item,
                    insert: None,
                    delete: None,
                }
            }
        }

        pub fn update_value(&mut self, elem_id: CompactIdLp, new_value: LoroValue, value_id: IdLp) {
            debug_assert!(elem_id.peer != PeerID::MAX);
            debug_assert!(!value_id.is_none());
            if let LoroValue::Container(c) = &new_value {
                self.child_container_to_elem.insert(c.clone(), elem_id);
            }

            if let Some(element) = self.elements.get_mut(&elem_id) {
                if let LoroValue::Container(c) = &element.value {
                    if element.value != new_value {
                        self.child_container_to_elem.remove(c);
                    }
                }
                element.value = new_value;
                element.value_id = value_id;
            } else {
                self.elements.insert(
                    elem_id,
                    Element {
                        value: new_value,
                        value_id,
                        pos: IdLp::NONE_ID,
                    },
                );
            }
        }

        /// push a new elem into the list
        pub fn push_inner(&mut self, list_item_id: IdFull, elem: Option<PushElemInfo>) {
            let pointed_by = elem.as_ref().map(|x| x.elem_id);
            if let Some(elem) = elem {
                if let LoroValue::Container(c) = &elem.value {
                    self.child_container_to_elem.insert(c.clone(), elem.elem_id);
                }
                debug_assert!(!elem.last_set_id.is_none());
                self.elements.insert(
                    elem.elem_id,
                    Element {
                        value: elem.value,
                        value_id: elem.last_set_id,
                        pos: list_item_id.idlp(),
                    },
                );
            }
            let cursor = self.list.push(ListItem {
                pointed_by,
                id: list_item_id,
            });
            self.id_to_list_leaf
                .insert(list_item_id.idlp(), cursor.leaf);
        }
    }

    pub(super) struct PushElemInfo {
        pub elem_id: CompactIdLp,
        pub value: LoroValue,
        pub last_set_id: IdLp,
    }

    pub(super) struct UpdateResultFromPosChange {
        /// Whether the list item is activated by this change.
        /// If so, the length of the list will be increased by 1 from the users' perspective.
        pub activate_new_list_item: bool,
        pub new_list_item_leaf: Option<LeafIndex>,
        /// Remove from the users' perspective. But they are still in the state.
        pub removed_old_list_item_leaf: Option<LeafIndex>,
    }

    /// This struct assumes we apply insert first and then delete.
    pub(super) struct EventPosInfo {
        pub activate_new_list_item: bool,
        pub insert: Option<usize>,
        pub delete: Option<usize>,
    }
}

impl MovableListState {
    pub fn new(idx: ContainerIdx) -> Self {
        Self {
            idx,
            inner: InnerState::new(),
        }
    }

    #[inline]
    fn list(&self) -> &BTree<MovableListTreeTrait> {
        self.inner.list()
    }

    #[inline]
    fn elements(&self) -> &FxHashMap<CompactIdLp, Element> {
        self.inner.elements()
    }

    /// Return whether the list item is activated by this change.
    ///
    /// If so, the length of the list will be increased by 1 from the users' perspective.
    ///
    /// It's false when the list item is already activated or deleted.
    #[must_use]
    fn create_new_elem(
        &mut self,
        elem_id: CompactIdLp,
        new_pos: IdLp,
        new_value: LoroValue,
        value_id: IdLp,
    ) -> UpdateResultFromPosChange {
        self.inner.update_value(elem_id, new_value, value_id);
        self.inner.update_pos(elem_id, new_pos, false)
    }

    /// Return the values that are activated by the insertions of the list items, and their elem ids.
    ///
    /// The activation means there were elements that already points to the list items
    /// inserted by this op.
    fn list_insert_batch(&mut self, index: usize, items: impl Iterator<Item = IdFull>) {
        for (i, item) in items.enumerate() {
            self.inner.insert_list_item(index + i, item);
        }
    }

    /// Get the length defined by op, where the length includes the ones that are not being pointed at (moved, invisible to users).
    #[allow(unused)]
    fn op_len(&self) -> usize {
        self.inner.list().root_cache().include_dead_len as usize
    }

    fn mov(
        &mut self,
        from_index: usize,
        to_index: usize,
        elem_id: IdLp,
        new_pos_id: IdFull,
        kind: IndexType,
    ) {
        // PERF: can be optimized by inlining try update
        if cfg!(debug_assertions) {
            let item = self.inner.get_list_item_at(from_index, kind).unwrap();
            assert_eq!(item.pointed_by, Some(elem_id.compact()));
        }

        self.inner.insert_list_item(
            if to_index > from_index {
                to_index + 1
            } else {
                to_index
            },
            new_pos_id,
        );
        let _ = self
            .inner
            .update_pos(elem_id.compact(), new_pos_id.idlp(), true);
    }

    pub(crate) fn get(&self, index: usize, kind: IndexType) -> Option<&LoroValue> {
        if index >= self.len() {
            return None;
        }

        let item = self.inner.get_list_item_at(index, kind)?;
        let elem = item.pointed_by?;
        self.inner.elements().get(&elem).map(|x| &x.value)
    }

    pub(crate) fn get_elem_at_given_pos(
        &self,
        index: usize,
        kind: IndexType,
    ) -> Option<(CompactIdLp, &Element)> {
        self.inner.get_list_item_at(index, kind).and_then(|x| {
            x.pointed_by
                .map(|pointed_by| (pointed_by, self.inner.elements().get(&pointed_by).unwrap()))
        })
    }

    pub(crate) fn get_list_id_at(&self, index: usize, kind: IndexType) -> Option<ID> {
        self.inner.get_list_item_at(index, kind).map(|x| x.id.id())
    }

    pub(crate) fn get_elem_id_at(&self, index: usize, kind: IndexType) -> Option<CompactIdLp> {
        self.inner
            .get_list_item_at(index, kind)
            .and_then(|x| x.pointed_by)
    }

    pub(crate) fn convert_index(
        &self,
        index: usize,
        from: IndexType,
        to: IndexType,
    ) -> Option<usize> {
        let len = self.inner.len_kind(from);
        if index == len {
            return Some(self.inner.len_kind(to));
        }

        if index > len {
            return None;
        }

        let c = match from {
            IndexType::ForOp => self.inner.list().query::<OpLenQuery>(&index).unwrap(),
            IndexType::ForUser => self.inner.list().query::<UserLenQuery>(&index).unwrap(),
        };

        Some(self.inner.get_index_of(c.cursor.leaf, to) as usize)
    }

    fn get_list_item(&self, id: IdLp) -> Option<&ListItem> {
        self.inner.get_list_item_by_id(id)
    }

    /// Get the user index of elem
    ///
    /// If we cannot find the list item in the list, we will return None.
    fn get_index_of_elem(&self, elem_id: CompactIdLp) -> Option<usize> {
        let elem = self.inner.elements().get(&elem_id)?;
        self.inner.get_list_item_index(elem.pos, IndexType::ForUser)
    }

    /// Debug check the consistency between the list and the elements
    ///
    /// We need to ensure that:
    /// - Every element's pos is in the list, and it has a `pointed_by` value that points
    ///   back to the element
    /// - Every list item's `pointed_by` value points to an element in the elements, and
    ///   the element has the correct `pos` value
    #[cfg(any(debug_assertions, test))]
    #[allow(unused)]
    pub(crate) fn check_consistency(&self) {
        for (id, elem) in self.inner.elements().iter() {
            let item = self
                .get_list_item(id.to_id())
                .expect("Elem's pos should be in the list");
            assert_eq!(item.pointed_by.unwrap(), *id);
        }

        for item in self.inner.list().iter() {
            if let Some(elem_id) = item.pointed_by {
                let elem = self.inner.elements().get(&elem_id).unwrap();
                assert_eq!(elem.pos, item.id.idlp());
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &LoroValue> {
        // PERF: can be optimized
        (0..self.len()).map(move |i| self.get(i, IndexType::ForUser).unwrap())
    }

    pub fn len(&self) -> usize {
        self.inner.list().root_cache().user_len as usize
    }

    fn to_vec(&self) -> Vec<LoroValue> {
        self.iter().cloned().collect_vec()
    }

    pub(crate) fn get_index_of_id(&self, id: ID) -> Option<usize> {
        let mut user_index = 0;
        for item in self.list().iter() {
            if item.id.peer == id.peer && item.id.counter == id.counter {
                return Some(user_index);
            }

            user_index += if item.pointed_by.is_some() { 1 } else { 0 };
        }
        None
    }

    fn get_value_inner(&self) -> Vec<LoroValue> {
        let list = self
            .inner
            .list()
            .iter_with_filter(|x| (x.user_len > 0, 0))
            .filter_map(|(_, item)| {
                item.pointed_by
                    .map(|eid| self.elements()[&eid].value.clone())
            })
            .collect();
        list
    }

    pub(crate) fn get_list_item_id_at(&self, pos: usize) -> Option<IdFull> {
        let item = self.inner.get_list_item_at(pos, IndexType::ForUser);
        item.map(|x| x.id)
    }
}

impl ContainerState for MovableListState {
    fn container_idx(&self) -> ContainerIdx {
        self.idx
    }

    fn estimate_size(&self) -> usize {
        self.len() * 8
    }

    fn is_state_empty(&self) -> bool {
        self.list().is_empty() && self.elements().is_empty()
    }

    #[instrument(skip_all)]
    fn apply_diff_and_convert(
        &mut self,
        diff: InternalDiff,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        let InternalDiff::MovableList(diff) = diff else {
            unreachable!()
        };

        if cfg!(debug_assertions) {
            self.inner.check_consistency();
        }

        let start_value = if cfg!(debug_assertions) {
            Some(self.get_value())
        } else {
            None
        };

        let id = arena.idx_to_id(self.idx).unwrap();
        let s = trace_span!("ListState", "ListState.id = {:?}", id);
        let _e = s.enter();
        debug!("InternalDiff for Movable {:#?}", &diff);

        let mut event: ListDiff = DeltaRope::new();
        let mut maybe_moved: FxHashMap<CompactIdLp, (usize, LoroValue)> = FxHashMap::default();

        {
            // apply deletions and calculate `maybe_moved`
            let mut index = 0;
            for delta_item in diff.list.iter() {
                match delta_item {
                    DeltaItem::Retain {
                        retain,
                        attributes: _,
                    } => {
                        index += retain;
                    }
                    DeltaItem::Insert { .. } => {}
                    DeltaItem::Delete {
                        delete,
                        attributes: _,
                    } => {
                        let mut user_index = self
                            .convert_index(index, IndexType::ForOp, IndexType::ForUser)
                            .unwrap();
                        let user_index_end = self
                            .convert_index(index + delete, IndexType::ForOp, IndexType::ForUser)
                            .unwrap();
                        event.compose(
                            &DeltaRopeBuilder::new()
                                .retain(user_index, Default::default())
                                .delete(user_index_end - user_index)
                                .build(),
                        );
                        self.inner.list_drain(index..index + delete, |id, elem| {
                            maybe_moved.insert(id, (user_index, elem.value.clone()));
                            user_index += 1;
                        });
                        assert_eq!(user_index, user_index_end);
                    }
                }
            }
        }

        {
            // apply list item changes
            let mut index = 0;
            for delta_item in diff.list.into_iter() {
                match delta_item {
                    DeltaItem::Retain {
                        retain,
                        attributes: _,
                    } => {
                        index += retain;
                    }
                    DeltaItem::Insert {
                        insert,
                        attributes: _,
                    } => {
                        let len = insert.len();
                        self.list_insert_batch(index, insert.into_iter());
                        index += len;
                    }
                    DeltaItem::Delete { .. } => {}
                }
            }
        }

        {
            // Apply element changes
            //
            // In this block, we need to handle the events generated from the following sources:
            //
            // - The change of elem's pos that activated the list item. This creates an insert event.
            // - The change of elem's value. This create a deletion and an insert event.
            //
            // It doesn't need to worry about the deletion of the list item, because it's handled by the list diff.

            for (elem_id, delta_item) in diff.elements.into_iter() {
                let crate::delta::ElementDelta {
                    pos,
                    pos_updated: _,
                    value,
                    value_updated,
                    value_id,
                } = delta_item;
                // Element may be dropped after snapshot encoding,
                // so we need to check which kind of update we need to do

                match self.inner.elements().get(&elem_id).cloned() {
                    Some(elem) => {
                        // Update value if needed
                        if elem.value != value {
                            maybe_moved.remove(&elem_id);
                            self.inner.update_value(elem_id, value.clone(), value_id);
                            let index = self.get_index_of_elem(elem_id);
                            if let Some(index) = index {
                                event.compose(
                                    &DeltaRopeBuilder::new()
                                        .retain(index, Default::default())
                                        .delete(1)
                                        .insert(
                                            ArrayVec::from([ValueOrHandler::from_value(
                                                value, arena, txn, state,
                                            )]),
                                            ListDeltaMeta { from_move: false },
                                        )
                                        .build(),
                                )
                            }
                        }

                        // Update pos if needed
                        if elem.pos != pos {
                            // don't need to update old list item, because it's handled by list diff already
                            let result = self.inner.update_pos(elem_id, pos, false);
                            let result = self.inner.convert_update_to_event_pos(result);
                            if let Some(new_index) = result.insert {
                                let new_value =
                                    self.elements().get(&elem_id).unwrap().value.clone();
                                let from_delete = if let Some((_elem_index, elem_old_value)) =
                                    maybe_moved.remove(&elem_id)
                                {
                                    elem_old_value == new_value
                                } else {
                                    false
                                };
                                let new_delta: ListDiff = DeltaRopeBuilder::new()
                                    .retain(new_index, Default::default())
                                    .insert(
                                        ArrayVec::from([ValueOrHandler::from_value(
                                            new_value, arena, txn, state,
                                        )]),
                                        ListDeltaMeta {
                                            from_move: (result.delete.is_some() && !value_updated)
                                                || from_delete,
                                        },
                                    )
                                    .build();
                                event.compose(&new_delta);
                            }
                            if let Some(del_index) = result.delete {
                                event.compose(
                                    &DeltaRopeBuilder::new()
                                        .retain(del_index, Default::default())
                                        .delete(1)
                                        .build(),
                                );
                            }
                            if !result.activate_new_list_item {
                                // not matched list item found, remove directly
                                self.inner.remove_elem_by_id(&elem_id);
                            }
                        }
                    }
                    None => {
                        // Need to create new element
                        let result = self.create_new_elem(elem_id, pos, value.clone(), value_id);
                        // Composing events
                        let result = self.inner.convert_update_to_event_pos(result);
                        // Create event for pos change and value change
                        if let Some(index) = result.insert {
                            let from_delete = if let Some((_elem_index, elem_old_value)) =
                                maybe_moved.remove(&elem_id)
                            {
                                elem_old_value == value
                            } else {
                                false
                            };
                            event.compose(
                                &DeltaRopeBuilder::new()
                                    .retain(index, Default::default())
                                    .insert(
                                        ArrayVec::from([ValueOrHandler::from_value(
                                            value, arena, txn, state,
                                        )]),
                                        ListDeltaMeta {
                                            from_move: (result.delete.is_some() && !value_updated)
                                                || from_delete,
                                        },
                                    )
                                    .build(),
                            )
                        }
                        if let Some(index) = result.delete {
                            event.compose(
                                &DeltaRopeBuilder::new()
                                    .retain(index, Default::default())
                                    .delete(1)
                                    .build(),
                            );
                        }
                        if !result.activate_new_list_item {
                            // not matched list item found, remove directly
                            self.inner.remove_elem_by_id(&elem_id);
                        }
                    }
                }
            }
        }

        {
            // Remove redundant elements

            // We now know that the elements inside `maybe_moved` are actually deleted.
            // So we can safely remove them.
            let redundant = maybe_moved;
            for (elem_id, _) in redundant.iter() {
                self.inner.remove_elem_by_id(elem_id);
            }
        }

        if cfg!(debug_assertions) {
            self.inner.check_consistency();
            let start_value = start_value.unwrap();
            let mut end_value = start_value.clone();
            debug!("start_value = {:#?} \nevent = {:#?}", &start_value, &event);
            end_value.apply_diff_shallow(&[Diff::List(event.clone())]);
            let cur_value = self.get_value();
            assert_eq!(
                end_value, cur_value,
                "start_value={:#?} event={:#?} new_state={:#?} but the end_value={:#?}",
                start_value, event, cur_value, end_value
            );
        }

        debug!(
            "MovableListState::apply_diff_and_convert: ans={:#?}",
            &event
        );
        Diff::List(event)
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

    #[instrument(skip_all)]
    fn apply_local_op(&mut self, op: &RawOp, _: &Op) -> LoroResult<()> {
        match op.content.as_list().unwrap() {
            ListOp::Insert { slice, pos } => match slice {
                ListSlice::RawData(list) => {
                    let mut a;
                    let mut b;
                    let v: &mut dyn Iterator<Item = &LoroValue>;
                    match list {
                        std::borrow::Cow::Borrowed(list) => {
                            a = Some(list.iter());
                            v = a.as_mut().unwrap();
                        }
                        std::borrow::Cow::Owned(list) => {
                            b = Some(list.iter());
                            v = b.as_mut().unwrap();
                        }
                    }

                    for (i, x) in v.enumerate() {
                        let elem_id = op.idlp().inc(i as i32).try_into().unwrap();
                        let pos_id = op.id_full().inc(i as i32);
                        self.inner.insert_list_item(*pos + i, pos_id);
                        let _ = self.create_new_elem(
                            elem_id,
                            pos_id.idlp(),
                            x.clone(),
                            elem_id.to_id(),
                        );
                    }
                }
                _ => unreachable!(),
            },
            ListOp::Delete(span) => {
                self.inner
                    .list_drain(span.start() as usize..span.end() as usize, |_, _| {});
            }
            ListOp::Move { from, to, elem_id } => {
                self.mov(
                    *from as usize,
                    *to as usize,
                    *elem_id,
                    op.id_full(),
                    IndexType::ForOp,
                );
            }
            ListOp::Set { elem_id, value } => {
                self.inner
                    .update_value(elem_id.compact(), value.clone(), op.idlp());
            }
            ListOp::StyleStart { .. } | ListOp::StyleEnd => unreachable!(),
        }

        Ok(())
    }

    fn to_diff(
        &mut self,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        Diff::List(
            DeltaRopeBuilder::new()
                .insert_many(
                    self.to_vec()
                        .into_iter()
                        .map(|v| ValueOrHandler::from_value(v, arena, txn, state)),
                    Default::default(),
                )
                .build(),
        )
    }

    fn get_value(&mut self) -> LoroValue {
        let list = self.get_value_inner();
        LoroValue::List(Arc::new(list))
    }

    /// Get the index of the child container
    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        self.inner
            .get_child_index(id, IndexType::ForUser)
            .map(Index::Seq)
    }

    #[allow(unused)]
    fn get_child_containers(&self) -> Vec<ContainerID> {
        self.inner
            .child_container_to_elem()
            .iter()
            .filter_map(|(c, elem_id)| {
                let elem = self.elements().get(elem_id)?;
                if elem.value.as_container() != Some(c) {
                    None
                } else {
                    Some(c.clone())
                }
            })
            .collect_vec()
    }

    fn encode_snapshot(&self, mut encoder: StateSnapshotEncoder) -> Vec<u8> {
        // We need to encode all the list items (including the ones that are not being pointed at)
        // We also need to encode the elements' ids, values and last set ids as long as the element has a
        // valid pos pointer (a pointer is valid when the pointee is in the list).

        // But we can infer the element's id, the value by the `last set id` directly.
        // Because they are included in the corresponding op.

        let len = self.len();
        let mut items = Vec::with_capacity(len);
        // starts with a sentinel value. The num of `invisible_list_item` may be updated later
        items.push(EncodedItem {
            pos_id_eq_elem_id: true,
            invisible_list_item: 0,
        });
        let mut ids = Vec::new();
        for item in self.list().iter() {
            if let Some(elem_id) = item.pointed_by {
                let eq = elem_id.to_id() == item.id.idlp();
                items.push(EncodedItem {
                    invisible_list_item: 0,
                    pos_id_eq_elem_id: eq,
                });
                if !eq {
                    ids.push(EncodedId {
                        peer_idx: encoder.register_peer(item.id.peer),
                        lamport: item.id.lamport,
                    });
                }
                let elem = self.elements().get(&elem_id).unwrap();
                encoder.encode_op(elem.value_id.into(), || unimplemented!());
            } else {
                items.last_mut().unwrap().invisible_list_item += 1;
                ids.push(EncodedId {
                    peer_idx: encoder.register_peer(item.id.peer),
                    lamport: item.id.lamport,
                });
            }
        }

        let out = EncodedSnapshot { items, ids };
        serde_columnar::to_vec(&out).unwrap()
    }

    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext) {
        let iter = serde_columnar::iter_from_bytes::<EncodedSnapshot>(ctx.blob).unwrap();
        let item_iter = iter.items;
        let mut item_ids = iter.ids;
        let last_set_op_iter = ctx.ops;
        let mut is_first = true;

        for item in item_iter {
            let EncodedItem {
                invisible_list_item,
                pos_id_eq_elem_id,
                // FIXME: replace with a result return
            } = item.unwrap();

            // the first one don't need to read op, it only needs to read the invisible list items
            if !is_first {
                let last_set_op = last_set_op_iter.next().unwrap();
                let idlp = last_set_op.id_full().idlp();
                let mut get_pos_id_full = |elem_id: IdLp| {
                    let pos_id = if pos_id_eq_elem_id {
                        elem_id
                    } else {
                        let id = item_ids.next().unwrap().unwrap();
                        IdLp::new(ctx.peers[id.peer_idx], id.lamport)
                    };
                    let pos_o_id = ctx.oplog.idlp_to_id(pos_id).unwrap();
                    IdFull {
                        peer: pos_id.peer,
                        lamport: pos_id.lamport,
                        counter: pos_o_id.counter,
                    }
                };
                match &last_set_op.op.content {
                    crate::op::InnerContent::List(l) => match l {
                        crate::container::list::list_op::InnerListOp::Insert { slice, pos: _ } => {
                            for (i, v) in ctx
                                .oplog
                                .arena
                                .iter_value_slice(slice.to_range())
                                .enumerate()
                            {
                                let elem_id = idlp.inc(i as i32);
                                let pos_full_id = get_pos_id_full(elem_id);
                                self.inner.push_inner(
                                    pos_full_id,
                                    Some(PushElemInfo {
                                        elem_id: elem_id.compact(),
                                        value: v,
                                        last_set_id: elem_id,
                                    }),
                                );
                            }
                        }
                        crate::container::list::list_op::InnerListOp::Set { elem_id, value } => {
                            let pos_full_id = get_pos_id_full(*elem_id);
                            self.inner.push_inner(
                                pos_full_id,
                                Some(PushElemInfo {
                                    elem_id: elem_id.compact(),
                                    value: value.clone(),
                                    last_set_id: idlp,
                                }),
                            );
                        }
                        _ => unreachable!(),
                    },
                    _ => unreachable!(),
                }
            }

            is_first = false;
            for _ in 0..invisible_list_item {
                let id = item_ids.next().unwrap().unwrap();
                let pos_id = IdLp::new(ctx.peers[id.peer_idx], id.lamport);
                let pos_o_id = ctx.oplog.idlp_to_id(pos_id).unwrap();
                let pos_id_full = IdFull {
                    peer: pos_id.peer,
                    lamport: pos_id.lamport,
                    counter: pos_o_id.counter,
                };
                self.inner.push_inner(pos_id_full, None);
            }
        }

        assert!(item_ids.next().is_none());
        assert!(last_set_op_iter.next().is_none());
    }
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone, Copy)]
struct EncodedItem {
    #[columnar(strategy = "DeltaRle")]
    invisible_list_item: usize,
    #[columnar(strategy = "BoolRle")]
    pos_id_eq_elem_id: bool,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct EncodedId {
    #[columnar(strategy = "DeltaRle")]
    peer_idx: usize,
    #[columnar(strategy = "DeltaRle")]
    lamport: u32,
}

#[columnar(ser, de)]
struct EncodedSnapshot {
    #[columnar(class = "vec", iter = "EncodedItem")]
    items: Vec<EncodedItem>,
    #[columnar(class = "vec", iter = "EncodedId")]
    ids: Vec<EncodedId>,
}

#[cfg(test)]
mod test {
    use crate::{HandlerTrait, LoroDoc, ToJson};
    use serde_json::json;

    #[test]
    fn basic_handler_ops() {
        let doc = LoroDoc::new_auto_commit();
        let list = doc.get_movable_list("list");
        list.insert(0, 0).unwrap();
        list.insert(1, 1).unwrap();
        list.insert(2, 2).unwrap();
        assert_eq!(list.get_value().to_json_value(), json!([0, 1, 2]));
        list.mov(0, 1).unwrap();
        assert_eq!(list.get_value().to_json_value(), json!([1, 0, 2]));
        list.mov(2, 0).unwrap();
        assert_eq!(list.get_value().to_json_value(), json!([2, 1, 0]));
        list.delete(0, 2).unwrap();
        assert_eq!(list.get_value().to_json_value(), json!([0]));
        list.insert(0, 9).unwrap();
        assert_eq!(list.get_value().to_json_value(), json!([9, 0]));
        list.delete(0, 2).unwrap();
        assert_eq!(list.get_value().to_json_value(), json!([]));
    }

    #[test]
    fn basic_sync() {
        let doc = LoroDoc::new_auto_commit();
        let list = doc.get_movable_list("list");
        list.insert(0, 1).unwrap();
        list.insert(1, 0).unwrap();
        list.mov(0, 1).unwrap();
        {
            let doc_b = LoroDoc::new_auto_commit();
            doc_b.import(&doc.export_from(&Default::default())).unwrap();
            assert_eq!(
                doc_b.get_deep_value().to_json_value(),
                json!({
                    "list": [0, 1]
                })
            );
        }
        list.mov(1, 0).unwrap();
        assert_eq!(
            doc.get_deep_value().to_json_value(),
            json!({
                "list": [1, 0]
            })
        );
        {
            let doc_b = LoroDoc::new_auto_commit();
            doc_b.import(&doc.export_from(&Default::default())).unwrap();
            assert_eq!(
                doc_b.get_deep_value().to_json_value(),
                json!({
                    "list": [1, 0]
                })
            );
        }
        list.mov(0, 1).unwrap();
        list.insert(2, 3).unwrap();
        list.set(2, 2).unwrap();
        assert_eq!(
            doc.get_deep_value().to_json_value(),
            json!({
                "list": [0, 1, 2]
            })
        );
        {
            let doc_b = LoroDoc::new_auto_commit();
            doc_b.import(&doc.export_from(&Default::default())).unwrap();
            assert_eq!(
                doc_b.get_deep_value().to_json_value(),
                json!({
                    "list": [0, 1, 2]
                })
            );
        }
        {
            let doc_b = LoroDoc::new_auto_commit();
            doc_b.import(&doc.export_snapshot()).unwrap();
            assert_eq!(
                doc_b.get_deep_value().to_json_value(),
                json!({
                    "list": [0, 1, 2]
                })
            );
        }
    }
}
