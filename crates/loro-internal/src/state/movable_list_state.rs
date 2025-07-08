use itertools::Itertools;
use loro_delta::{array_vec::ArrayVec, DeltaRope, DeltaRopeBuilder};
use serde_columnar::columnar;
use std::sync::Weak;
use tracing::{instrument, warn};

use fxhash::FxHashMap;
use generic_btree::BTree;
use loro_common::{CompactIdLp, ContainerID, IdFull, IdLp, LoroResult, LoroValue, PeerID, ID};

use crate::{
    configure::Configure,
    container::{idx::ContainerIdx, list::list_op::ListOp},
    delta::DeltaItem,
    diff_calc::DiffMode,
    encoding::{StateSnapshotDecodeContext, StateSnapshotEncoder},
    event::{Diff, Index, InternalDiff, ListDeltaMeta},
    handler::ValueOrHandler,
    op::{ListSlice, Op, RawOp},
    state::movable_list_state::inner::PushElemInfo,
    undo::DiffBatch,
    ListDiff, LoroDocInner,
};

use self::{
    inner::{InnerState, UpdateResultFromPosChange},
    list_item_tree::{MovableListTreeTrait, OpLenQuery, UserLenQuery},
};

use super::{ApplyLocalOpReturn, ContainerState, DiffApplyContext};

#[derive(Debug, Clone)]
pub struct MovableListState {
    idx: ContainerIdx,
    inner: InnerState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    pointed_by: Option<CompactIdLp>,
    pub(crate) id: IdFull,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Element {
    pub(crate) value: LoroValue,
    pub(crate) value_id: IdLp,
    pub(crate) pos: IdLp,
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
        fn try_insert(&mut self, _pos: usize, elem: Self) -> Result<(), Self>
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

    impl PartialEq for InnerState {
        fn eq(&self, other: &Self) -> bool {
            let v = self.id_to_list_leaf == other.id_to_list_leaf
                && self.elements == other.elements
                && self.child_container_to_elem == other.child_container_to_elem;
            if !v {
                false
            } else {
                self.list.iter().zip(other.list.iter()).all(|(a, b)| a == b)
            }
        }
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
            if !cfg!(debug_assertions) {
                return;
            }

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
                IndexType::ForUser => self.list.query::<UserLenQuery>(&pos)?,
                IndexType::ForOp => self.list.query::<OpLenQuery>(&pos)?,
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

        pub fn contains_child_container(&self, id: &ContainerID) -> bool {
            self.get_child_index(id, IndexType::ForUser).is_some()
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
        ///
        /// We also remove the elements that are pointed by the list items.
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
                self.list.update_leaf(*leaf, |list_item| {
                    debug_assert!(
                        list_item.pointed_by.is_none(),
                        "list_item was pointed by {:?} but need to be changed to {:?}",
                        list_item.pointed_by,
                        elem_id
                    );
                    ans.activate_new_list_item = list_item.pointed_by.is_none();
                    list_item.pointed_by = Some(elem_id);
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

        pub fn update_value(
            &mut self,
            elem_id: CompactIdLp,
            new_value: LoroValue,
            value_id: IdLp,
        ) -> Option<LoroValue> {
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
                let old_value = std::mem::replace(&mut element.value, new_value);
                element.value_id = value_id;
                Some(old_value)
            } else {
                self.elements.insert(
                    elem_id,
                    Element {
                        value: new_value,
                        value_id,
                        pos: IdLp::NONE_ID,
                    },
                );
                None
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
    pub(crate) fn elements(&self) -> &FxHashMap<CompactIdLp, Element> {
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

    pub(crate) fn get_list_item(&self, id: IdLp) -> Option<&ListItem> {
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

    pub fn iter_with_last_move_id_and_elem_id(
        &self,
    ) -> impl Iterator<Item = (IdFull, CompactIdLp, &LoroValue)> {
        self.inner.list().iter().filter_map(|list_item| {
            if let Some(elem_id) = list_item.pointed_by.as_ref() {
                let elem = self.inner.elements().get(elem_id).unwrap();
                Some((list_item.id, *elem_id, &elem.value))
            } else {
                None
            }
        })
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

    #[allow(unused)]
    fn check_get_child_index_correctly(&mut self) {
        let value = self.get_value();
        let list = value.into_list().unwrap();
        for (i, item) in list.iter().enumerate() {
            if let LoroValue::Container(c) = item {
                let child_index = self.get_child_index(c).expect("cannot find child index");
                assert_eq!(child_index.into_seq().unwrap(), i);
            }
        }
    }

    pub fn get_creator_at(&self, pos: usize) -> Option<PeerID> {
        self.inner
            .get_list_item_at(pos, IndexType::ForUser)
            .and_then(|x| x.pointed_by.map(|x| x.peer))
    }

    pub fn get_last_mover_at(&self, pos: usize) -> Option<PeerID> {
        self.inner
            .get_list_item_at(pos, IndexType::ForUser)
            .map(|x| x.id.peer)
    }

    pub fn get_last_editor_at(&self, pos: usize) -> Option<PeerID> {
        self.inner
            .get_list_item_at(pos, IndexType::ForUser)
            .and_then(|x| {
                x.pointed_by
                    .and_then(|x| self.inner.elements().get(&x).map(|x| x.value_id.peer))
            })
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

    // How we apply the diff is coupled with the [DiffMode] we used to calculate the diff.
    // So be careful when you modify this function.
    #[instrument(skip_all)]
    fn apply_diff_and_convert(
        &mut self,
        diff: InternalDiff,
        DiffApplyContext { doc, mode }: DiffApplyContext,
    ) -> Diff {
        let InternalDiff::MovableList(mut diff) = diff else {
            unreachable!()
        };

        // let start_value = self.get_value();
        if cfg!(debug_assertions) {
            self.inner.check_consistency();
        }

        let mut event: ListDiff = DeltaRope::new();
        let mut maybe_moved: FxHashMap<CompactIdLp, (usize, LoroValue)> = FxHashMap::default();
        let need_compare = matches!(mode, DiffMode::Import);

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
                        self.inner
                            .list_drain(index..index + delete, |elem_id, elem| {
                                maybe_moved.insert(elem_id, (user_index, elem.value.clone()));
                                if !matches!(mode, DiffMode::Checkout) {
                                    if let Some(new_elem) = diff.elements.get_mut(&elem_id) {
                                        if new_elem.value_id.is_none() {
                                            new_elem.value = elem.value.clone();
                                            new_elem.value_id = Some(elem.value_id);
                                            new_elem.value_updated = false;
                                        }

                                        if new_elem.pos.is_none() {
                                            new_elem.pos = Some(elem.pos);
                                        }
                                    }
                                }

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
            let doc = &doc.upgrade().unwrap();
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
                    value,
                    value_updated,
                    value_id,
                } = delta_item;
                // Element may be dropped after snapshot encoding,
                // so we need to check which kind of update we need to do

                match self.inner.elements().get(&elem_id).cloned() {
                    Some(elem) => {
                        // Update value if needed
                        if value_id.is_some()
                            && elem.value != value
                            && (!need_compare || elem.value_id < value_id.unwrap())
                        {
                            maybe_moved.remove(&elem_id);
                            self.inner
                                .update_value(elem_id, value.clone(), value_id.unwrap());
                            let index = self.get_index_of_elem(elem_id);
                            if let Some(index) = index {
                                event.compose(
                                    &DeltaRopeBuilder::new()
                                        .retain(index, Default::default())
                                        .delete(1)
                                        .insert(
                                            ArrayVec::from([ValueOrHandler::from_value(
                                                value, doc,
                                            )]),
                                            ListDeltaMeta { from_move: false },
                                        )
                                        .build(),
                                )
                            }
                        }

                        // Update pos if needed
                        if pos.is_some()
                            && elem.pos != pos.unwrap()
                            && (!need_compare || elem.pos < pos.unwrap())
                        {
                            // don't need to update old list item, because it's handled by list diff already
                            let result = self.inner.update_pos(elem_id, pos.unwrap(), false);
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
                                            new_value, doc,
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
                        let result = self.create_new_elem(
                            elem_id,
                            pos.unwrap(),
                            value.clone(),
                            value_id.unwrap(),
                        );
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
                                        ArrayVec::from([ValueOrHandler::from_value(value, doc)]),
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

        // if cfg!(debug_assertions) {
        //     self.inner.check_consistency();
        //     let mut end_value = start_value.clone();
        //     end_value.apply_diff_shallow(&[Diff::List(event.clone())]);
        //     let cur_value = self.get_value();
        //     assert_eq!(
        //         end_value, cur_value,
        //         "start_value={:#?} event={:#?} new_state={:#?} but the end_value={:#?}",
        //         start_value, event, cur_value, end_value
        //     );
        //     self.check_get_child_index_correctly();
        // }

        Diff::List(event)
    }

    // How we apply the diff is coupled with the [DiffMode] we used to calculate the diff.
    // So be careful when you modify this function.
    fn apply_diff(&mut self, diff: InternalDiff, ctx: DiffApplyContext) {
        let _ = self.apply_diff_and_convert(diff, ctx);
    }

    #[instrument(skip_all)]
    fn apply_local_op(
        &mut self,
        op: &RawOp,
        _: &Op,
        undo_diff: Option<&mut DiffBatch>,
        doc: &Weak<LoroDocInner>,
    ) -> LoroResult<ApplyLocalOpReturn> {
        let mut ans: ApplyLocalOpReturn = Default::default();

        // Generate undo diff if requested
        if let Some(undo_batch) = undo_diff {
            if let Some(doc) = doc.upgrade() {
                if let Some(container_id) = doc.arena.get_container_id(self.idx) {
                    match op.content.as_list().unwrap() {
                        ListOp::Insert { slice, pos } => {
                            // For insert, undo is to delete the inserted items
                            let len = match slice {
                                ListSlice::RawData(list) => list.len(),
                                _ => unreachable!(),
                            };

                            let mut diff = ListDiff::default();
                            diff.push_retain(*pos as usize, Default::default());
                            diff.push_delete(len);

                            let undo_diff = Diff::List(diff);
                            undo_batch
                                .cid_to_events
                                .insert(container_id.clone(), undo_diff);
                            undo_batch.order.push(container_id.clone());
                        }
                        ListOp::Delete(span) => {
                            // For delete, undo is to insert the deleted values back
                            let range = span.start() as usize..span.end() as usize;
                            let mut deleted_values = ArrayVec::new();

                            // Collect the values that will be deleted
                            for i in range.clone() {
                                if let Some(value) = self.get(i, IndexType::ForOp) {
                                    let _ = deleted_values
                                        .push(ValueOrHandler::from_value(value.clone(), &doc));
                                }
                            }

                            if !deleted_values.is_empty() {
                                let mut diff = ListDiff::default();
                                diff.push_retain(range.start, Default::default());
                                diff.push_insert(deleted_values, ListDeltaMeta::default());

                                let undo_diff = Diff::List(diff);
                                undo_batch.push_with_transform(&container_id, undo_diff);
                            }
                        }
                        ListOp::Move { from, elem_id, .. } => {
                            // For move, undo is to move back to the original position
                            // We'll generate the undo move operation with from and to swapped
                            let mut diff = ListDiff::default();

                            // Find the element's current position (which will be its new position after the move)
                            // and generate a move back to the original position
                            if let Some(elem) = self.inner.elements().get(&elem_id.compact()) {
                                let value = elem.value.clone();

                                // The undo operation needs to track that this element should be moved back
                                // For now, we'll use a delete + insert pattern to simulate the move
                                // First retain to the current position (which will be after the move)
                                let current_pos = *from as usize;
                                diff.push_retain(current_pos, Default::default());
                                diff.push_delete(1);
                                let mut arr = ArrayVec::new();
                                let _ = arr.push(ValueOrHandler::from_value(value, &doc));
                                diff.push_insert(arr, ListDeltaMeta::default());

                                let undo_diff = Diff::List(diff);
                                undo_batch.push_with_transform(&container_id, undo_diff);
                            }
                        }
                        ListOp::Set {
                            elem_id,
                            value: _new_value,
                        } => {
                            // For set, undo is to restore the old value
                            // Get the old value before it's replaced
                            if let Some(elem) = self.inner.elements().get(&elem_id.compact()) {
                                let old_value = elem.value.clone();

                                // For set operation, we want to generate a replace operation that restores the old value
                                // We don't need to find the index as the element stays at the same position
                                let mut diff = ListDiff::default();
                                // Since this is a set operation on a movable list, we need to generate
                                // an undo that will restore the old value at the same position
                                let mut arr = ArrayVec::new();
                                let _ = arr.push(ValueOrHandler::from_value(old_value, &doc));
                                diff.push_replace(arr, ListDeltaMeta::default(), 0);

                                let undo_diff = Diff::List(diff);
                                undo_batch
                                    .cid_to_events
                                    .insert(container_id.clone(), undo_diff);
                                undo_batch.order.push(container_id.clone());
                            }
                        }
                        ListOp::StyleStart { .. } | ListOp::StyleEnd => unreachable!(),
                    }
                }
            }
        }

        // Apply the operation
        match op.content.as_list().unwrap() {
            ListOp::Insert { slice, pos } => match slice {
                ListSlice::RawData(list) => {
                    for (i, x) in list.as_ref().iter().enumerate() {
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
                    .list_drain(span.start() as usize..span.end() as usize, |_, elem| {
                        if let Some(c) = elem.value.as_container() {
                            ans.deleted_containers.push(c.clone());
                        }
                    });
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
                let old_value =
                    self.inner
                        .update_value(elem_id.compact(), value.clone(), op.idlp());
                if let Some(LoroValue::Container(c)) = old_value {
                    ans.deleted_containers.push(c);
                }
            }
            ListOp::StyleStart { .. } | ListOp::StyleEnd => unreachable!(),
        }

        Ok(ans)
    }

    fn to_diff(&mut self, doc: &Weak<LoroDocInner>) -> Diff {
        let doc = &doc.upgrade().unwrap();
        Diff::List(
            DeltaRopeBuilder::new()
                .insert_many(
                    self.to_vec()
                        .into_iter()
                        .map(|v| ValueOrHandler::from_value(v, doc)),
                    Default::default(),
                )
                .build(),
        )
    }

    fn get_value(&mut self) -> LoroValue {
        let list = self.get_value_inner();
        LoroValue::List(list.into())
    }

    /// Get the index of the child container
    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        self.inner
            .get_child_index(id, IndexType::ForUser)
            .map(Index::Seq)
    }

    fn contains_child(&self, id: &ContainerID) -> bool {
        self.inner.contains_child_container(id)
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

    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext) -> LoroResult<()> {
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
        Ok(())
    }

    fn fork(&self, _config: &Configure) -> Self {
        self.clone()
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

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone, Copy)]
struct EncodedItemForFastSnapshot {
    #[columnar(strategy = "DeltaRle")]
    invisible_list_item: usize,
    #[columnar(strategy = "BoolRle")]
    pos_id_eq_elem_id: bool,
    #[columnar(strategy = "BoolRle")]
    elem_id_eq_last_set_id: bool,
}

#[columnar(vec, ser, de, iterable)]
#[derive(Debug, Clone)]
struct EncodedIdFull {
    #[columnar(strategy = "DeltaRle")]
    peer_idx: usize,
    #[columnar(strategy = "DeltaRle")]
    counter: i32,
    #[columnar(strategy = "DeltaRle")]
    lamport_sub_counter: i32,
}

#[columnar(ser, de)]
struct EncodedFastSnapshot {
    #[columnar(class = "vec", iter = "EncodedItemForFastSnapshot")]
    items: Vec<EncodedItemForFastSnapshot>,
    #[columnar(class = "vec", iter = "EncodedIdFull")]
    list_item_ids: Vec<EncodedIdFull>,
    #[columnar(class = "vec", iter = "EncodedId")]
    elem_ids: Vec<EncodedId>,
    #[columnar(class = "vec", iter = "EncodedId")]
    last_set_ids: Vec<EncodedId>,
}

mod snapshot {
    use std::io::Read;

    use loro_common::{IdFull, IdLp, LoroValue, PeerID};

    use crate::{
        encoding::value_register::ValueRegister,
        state::{ContainerCreationContext, ContainerState, FastStateSnapshot},
    };

    use super::{
        inner::PushElemInfo, EncodedFastSnapshot, EncodedId, EncodedIdFull,
        EncodedItemForFastSnapshot, MovableListState,
    };

    impl FastStateSnapshot for MovableListState {
        /// Encodes the MovableListState into a compact binary format for fast snapshot storage and retrieval.
        ///
        /// The encoding format consists of:
        /// 1. The full value of the MovableListState, encoded using postcard serialization.
        /// 2. A series of EncodedItemForFastSnapshot structs representing each visible list item:
        ///    - invisible_list_item: Count of invisible items before this item (RLE encoded)
        ///    - pos_id_eq_elem_id: Boolean indicating if position ID equals element ID (RLE encoded)
        ///    - elem_id_eq_last_set_id: Boolean indicating if element ID equals last set ID (RLE encoded)
        /// 3. A series of EncodedIdFull structs for list item IDs:
        ///    - peer_idx: Index of the peer ID in a value register (delta-RLE encoded)
        ///    - counter: Operation counter (delta-RLE encoded)
        ///    - lamport: Lamport timestamp (delta-RLE encoded)
        /// 4. EncodedId structs for element IDs (when different from position ID)
        /// 5. EncodedId structs for last set IDs (when different from element ID)
        /// 6. A list of unique peer IDs used in the encoding
        fn encode_snapshot_fast<W: std::io::prelude::Write>(&mut self, mut w: W) {
            let value = self.get_value().into_list().unwrap();
            postcard::to_io(&*value, &mut w).unwrap();
            let mut peers: ValueRegister<PeerID> = ValueRegister::new();
            let len = self.len();
            let mut items = Vec::with_capacity(len);
            // starts with a sentinel value. The num of `invisible_list_item` may be updated later
            items.push(EncodedItemForFastSnapshot {
                pos_id_eq_elem_id: true,
                invisible_list_item: 0,
                elem_id_eq_last_set_id: true,
            });

            let mut list_item_ids = Vec::with_capacity(self.len());
            let mut elem_ids = Vec::new();
            let mut last_set_ids = Vec::new();
            for item in self.list().iter() {
                if let Some(elem_id) = item.pointed_by {
                    let elem = self.elements().get(&elem_id).unwrap();
                    let elem_eq_list_item = elem_id.to_id() == item.id.idlp();
                    let elem_id_eq_last_set_id = elem.value_id.compact() == elem_id;
                    items.push(EncodedItemForFastSnapshot {
                        invisible_list_item: 0,
                        pos_id_eq_elem_id: elem_eq_list_item,
                        elem_id_eq_last_set_id,
                    });

                    list_item_ids.push(super::EncodedIdFull {
                        peer_idx: peers.register(&item.id.peer),
                        counter: item.id.counter,
                        lamport_sub_counter: (item.id.lamport as i32 - item.id.counter),
                    });
                    if !elem_eq_list_item {
                        elem_ids.push(super::EncodedId {
                            peer_idx: peers.register(&elem_id.peer),
                            lamport: elem_id.lamport.get(),
                        })
                    }
                    if !elem_id_eq_last_set_id {
                        last_set_ids.push(super::EncodedId {
                            peer_idx: peers.register(&elem.value_id.peer),
                            lamport: elem.value_id.lamport,
                        })
                    }
                } else {
                    items.last_mut().unwrap().invisible_list_item += 1;
                    list_item_ids.push(super::EncodedIdFull {
                        peer_idx: peers.register(&item.id.peer),
                        counter: item.id.counter,
                        lamport_sub_counter: (item.id.lamport as i32 - item.id.counter),
                    });
                }
            }

            let peers = peers.unwrap_vec();
            leb128::write::unsigned(&mut w, peers.len() as u64).unwrap();
            for peer in peers {
                w.write_all(&peer.to_le_bytes()).unwrap();
            }

            let v = serde_columnar::to_vec(&EncodedFastSnapshot {
                items,
                list_item_ids,
                elem_ids,
                last_set_ids,
            })
            .unwrap();
            w.write_all(&v).unwrap();
        }

        fn decode_value(bytes: &[u8]) -> loro_common::LoroResult<(loro_common::LoroValue, &[u8])> {
            let (list_value, bytes) =
                postcard::take_from_bytes::<Vec<LoroValue>>(bytes).map_err(|_| {
                    loro_common::LoroError::DecodeError(
                        "Decode list value failed".to_string().into_boxed_str(),
                    )
                })?;
            Ok((list_value.into(), bytes))
        }

        fn decode_snapshot_fast(
            idx: crate::container::idx::ContainerIdx,
            (list_value, mut bytes): (loro_common::LoroValue, &[u8]),
            _ctx: ContainerCreationContext,
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

            let mut ans = MovableListState::new(idx);

            let iters = serde_columnar::iter_from_bytes::<EncodedFastSnapshot>(bytes).unwrap();
            let mut elem_iter = iters.elem_ids;
            let item_iter = iters.items;
            let mut list_item_id_iter = iters.list_item_ids;
            let mut last_set_id_iter = iters.last_set_ids;
            let mut is_first = true;

            let list_value = list_value.into_list().unwrap();
            let mut list_value_iter = list_value.iter();
            for item in item_iter {
                let EncodedItemForFastSnapshot {
                    invisible_list_item,
                    pos_id_eq_elem_id,
                    elem_id_eq_last_set_id,
                } = item.unwrap();

                if !is_first {
                    let EncodedIdFull {
                        peer_idx,
                        counter,
                        lamport_sub_counter,
                    } = list_item_id_iter.next().unwrap().unwrap();
                    let id_full = IdFull::new(
                        peers[peer_idx],
                        counter,
                        (lamport_sub_counter + counter) as u32,
                    );
                    let elem_id = if pos_id_eq_elem_id {
                        id_full.idlp()
                    } else {
                        let EncodedId { peer_idx, lamport } = elem_iter.next().unwrap().unwrap();
                        IdLp::new(peers[peer_idx], lamport)
                    };

                    let last_set_id = if elem_id_eq_last_set_id {
                        elem_id
                    } else {
                        let EncodedId { peer_idx, lamport } =
                            last_set_id_iter.next().unwrap().unwrap();
                        IdLp::new(peers[peer_idx], lamport)
                    };

                    let value = list_value_iter.next().unwrap();
                    ans.inner.push_inner(
                        id_full,
                        Some(PushElemInfo {
                            elem_id: elem_id.compact(),
                            value: value.clone(),
                            last_set_id,
                        }),
                    )
                }

                is_first = false;
                for _ in 0..invisible_list_item {
                    let EncodedIdFull {
                        peer_idx,
                        counter,
                        lamport_sub_counter,
                    } = list_item_id_iter.next().unwrap().unwrap();
                    let id_full = IdFull::new(
                        peers[peer_idx],
                        counter,
                        (counter + lamport_sub_counter) as u32,
                    );
                    ans.inner.push_inner(id_full, None);
                }
            }

            debug_assert!(elem_iter.next().is_none());
            debug_assert!(list_item_id_iter.next().is_none());
            debug_assert!(last_set_id_iter.next().is_none());
            debug_assert!(list_value_iter.next().is_none());

            Ok(ans)
        }
    }

    #[cfg(test)]
    mod test {

        use loro_common::{CompactIdLp, ContainerID, LoroValue, ID};

        use crate::container::idx::ContainerIdx;

        use super::*;

        #[test]
        fn test_movable_list_snapshot() {
            let mut list = MovableListState::new(ContainerIdx::from_index_and_type(
                0,
                loro_common::ContainerType::MovableList,
            ));

            list.inner.insert_list_item(0, IdFull::new(9, 9, 9));
            list.inner.insert_list_item(1, IdFull::new(0, 0, 0));
            let _ = list.create_new_elem(
                CompactIdLp::new(10, 10),
                IdLp {
                    lamport: 0,
                    peer: 0,
                },
                LoroValue::Container(ContainerID::new_normal(
                    ID::new(10, 10),
                    loro_common::ContainerType::Text,
                )),
                IdLp {
                    lamport: 1,
                    peer: 2,
                },
            );
            list.inner.insert_list_item(2, IdFull::new(1, 1, 1));
            list.inner.insert_list_item(3, IdFull::new(2, 2, 2));
            list.inner.insert_list_item(4, IdFull::new(3, 3, 3));
            let _ = list.create_new_elem(
                CompactIdLp::new(3, 8),
                IdLp {
                    lamport: 3,
                    peer: 3,
                },
                LoroValue::String("abc".into()),
                IdLp {
                    lamport: 4,
                    peer: 5,
                },
            );

            let mut bytes = Vec::new();
            list.encode_snapshot_fast(&mut bytes);
            assert!(bytes.len() <= 117, "{}", bytes.len());

            let (v, bytes) = MovableListState::decode_value(&bytes).unwrap();
            assert_eq!(
                v,
                vec![
                    LoroValue::Container(ContainerID::new_normal(
                        ID::new(10, 10),
                        loro_common::ContainerType::Text,
                    )),
                    LoroValue::String("abc".into()),
                ]
                .into()
            );
            let mut list2 = MovableListState::decode_snapshot_fast(
                ContainerIdx::from_index_and_type(0, loro_common::ContainerType::MovableList),
                (v.clone(), bytes),
                ContainerCreationContext {
                    configure: &Default::default(),
                    peer: 0,
                },
            )
            .unwrap();
            assert_eq!(&list2.get_value(), &v);
            assert_eq!(&list.inner, &list2.inner);
        }

        #[test]
        fn test_movable_list_snapshot_size() {
            let mut list = MovableListState::new(ContainerIdx::from_index_and_type(
                0,
                loro_common::ContainerType::MovableList,
            ));

            list.inner.insert_list_item(0, IdFull::new(0, 0, 0));
            let _ = list.create_new_elem(
                CompactIdLp::new(0, 0),
                IdLp {
                    lamport: 0,
                    peer: 0,
                },
                LoroValue::I64(0),
                IdLp {
                    lamport: 0,
                    peer: 0,
                },
            );

            list.inner.insert_list_item(1, IdFull::new(0, 1, 1));
            let _ = list.create_new_elem(
                CompactIdLp::new(0, 1),
                IdLp {
                    peer: 0,
                    lamport: 1,
                },
                LoroValue::I64(0),
                IdLp {
                    peer: 0,
                    lamport: 1,
                },
            );

            let mut bytes = Vec::new();
            list.encode_snapshot_fast(&mut bytes);
            assert!(bytes.len() <= 42, "{}", bytes.len());

            list.inner.insert_list_item(2, IdFull::new(0, 1, 2));
            let _ = list.create_new_elem(
                CompactIdLp::new(0, 2),
                IdLp {
                    peer: 0,
                    lamport: 2,
                },
                LoroValue::I64(0),
                IdLp {
                    peer: 0,
                    lamport: 2,
                },
            );
            let mut bytes = Vec::new();
            list.encode_snapshot_fast(&mut bytes);
            assert!(bytes.len() <= 47, "{}", bytes.len());
        }
    }
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
            doc_b.import(&doc.export_snapshot().unwrap()).unwrap();
            assert_eq!(
                doc_b.get_deep_value().to_json_value(),
                json!({
                    "list": [0, 1, 2]
                })
            );
        }
    }
}
