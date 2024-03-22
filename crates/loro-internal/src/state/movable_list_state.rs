use itertools::Itertools;
use serde_columnar::columnar;
use std::sync::{Arc, Mutex, Weak};
use tracing::{debug, instrument};

use fxhash::FxHashMap;
use generic_btree::{BTree, Cursor, LeafIndex, Query};
use loro_common::{CompactIdLp, ContainerID, IdFull, IdLp, LoroResult, LoroValue, ID};

use crate::{
    arena::SharedArena,
    container::{idx::ContainerIdx, list::list_op::ListOp},
    delta::{Delta, DeltaItem},
    encoding::{StateSnapshotDecodeContext, StateSnapshotEncoder},
    event::{Diff, Index, InternalDiff, ListDeltaMeta},
    handler::ValueOrHandler,
    op::{ListSlice, Op, RawOp},
    txn::Transaction,
    DocState,
};

use self::list_item_tree::{MovableListTreeTrait, OpLenQuery, UserLenQuery};

use super::ContainerState;

#[derive(Debug, Clone)]
pub struct MovableListState {
    idx: ContainerIdx,
    list: BTree<MovableListTreeTrait>,
    id_to_list_leaf: FxHashMap<IdLp, LeafIndex>,
    elements: FxHashMap<CompactIdLp, Element>,
}

#[derive(Debug, Clone)]
pub struct ListItem {
    pointed_by: Option<CompactIdLp>,
    id: IdFull,
}

#[derive(Debug, Clone)]
struct Element {
    value: LoroValue,
    value_id: IdLp,
    pos: IdLp,
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
        rle::{HasLength, Mergeable, Sliceable},
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

impl MovableListState {
    pub fn new(idx: ContainerIdx) -> Self {
        let list = BTree::new();
        Self {
            idx,
            list,
            id_to_list_leaf: FxHashMap::default(),
            elements: FxHashMap::default(),
        }
    }

    fn create_new_elem(&mut self, id: IdLp, new_pos: IdLp, new_value: LoroValue, value_id: IdLp) {
        self.update_elem_pos(id, new_pos, true, false);
        self.update_elem_value(id, new_value, value_id, true);
    }

    /// This update may not succeed if the given value_id is smaller than the existing value_id.
    ///
    /// Return whether the update is successful.
    fn update_elem_pos(
        &mut self,
        elem_id: IdLp,
        list_item_id: IdLp,
        force: bool,
        remove_old_list_item: bool,
    ) -> bool {
        let id = elem_id.try_into().unwrap();
        let mut old_item_id = None;
        if let Some(element) = self.elements.get_mut(&id) {
            if !force && element.pos > list_item_id {
                return false;
            }

            if element.pos != list_item_id {
                old_item_id = Some(element.pos);
                element.pos = list_item_id;
            }
        } else {
            self.elements.insert(
                id,
                Element {
                    value: LoroValue::Null,
                    value_id: IdLp::NONE_ID,
                    pos: list_item_id,
                },
            );
        }

        let leaf = self.id_to_list_leaf.get(&list_item_id).unwrap();
        self.list.update_leaf(*leaf, |elem| {
            let was_none = elem.pointed_by.is_none();
            elem.pointed_by = Some(elem_id.try_into().unwrap());
            if was_none {
                (true, None, None)
            } else {
                (false, None, None)
            }
        });

        if remove_old_list_item {
            if let Some(old) = old_item_id {
                if !old.is_none() {
                    let leaf = self.id_to_list_leaf.remove(&old).unwrap();
                    self.list.remove_leaf(Cursor { leaf, offset: 0 });
                }
            }
        } else if let Some(old) = old_item_id {
            if !old.is_none() {
                if let Some(leaf) = self.id_to_list_leaf.get(&old) {
                    let (still_valid, split) = self.list.update_leaf(*leaf, |item| {
                        item.pointed_by = None;
                        (true, None, None)
                    });
                    assert!(still_valid);
                    assert!(split.arr.is_empty());
                }
            }
        }

        true
    }

    /// This update may not succeed if the given value_id is smaller than the existing value_id.
    ///
    /// Return whether the update is successful.
    fn update_elem_value(
        &mut self,
        elem: IdLp,
        value: LoroValue,
        value_id: IdLp,
        force: bool,
    ) -> bool {
        let id = elem.try_into().unwrap();
        if let Some(element) = self.elements.get_mut(&id) {
            if !force && element.value_id > value_id {
                return false;
            }

            element.value = value;
            element.value_id = value_id;
        } else {
            self.elements.insert(
                id,
                Element {
                    value,
                    value_id,
                    pos: IdLp::NONE_ID,
                },
            );
        }

        true
    }

    fn list_insert(&mut self, index: usize, item: ListItem, kind: IndexType) {
        let id = item.id;
        let cursor = if index == self.len_kind(kind) {
            self.list.push(item)
        } else {
            match kind {
                IndexType::ForUser => self.list.insert::<UserLenQuery>(&index, item).0,
                IndexType::ForOp => self.list.insert::<OpLenQuery>(&index, item).0,
            }
        };
        self.id_to_list_leaf.insert(id.idlp(), cursor.leaf);
    }

    fn list_insert_batch(
        &mut self,
        index: usize,
        items: impl Iterator<Item = ListItem>,
        kind: IndexType,
    ) {
        for (i, item) in items.enumerate() {
            self.list_insert(index + i, item, kind);
        }
    }

    /// Get the length defined by op, where the length includes the ones that are not being pointed at (moved, invisible to users).
    #[allow(unused)]
    fn op_len(&self) -> usize {
        self.list.root_cache().include_dead_len as usize
    }

    fn len_kind(&self, kind: IndexType) -> usize {
        match kind {
            IndexType::ForUser => self.list.root_cache().user_len as usize,
            IndexType::ForOp => self.list.root_cache().include_dead_len as usize,
        }
    }

    #[instrument(skip(self))]
    fn list_drain(&mut self, range: std::ops::Range<usize>, kind: IndexType) {
        match kind {
            IndexType::ForUser => {
                for item in Self::drain_by_query::<UserLenQuery>(&mut self.list, range) {
                    self.id_to_list_leaf.remove(&item.id.idlp());
                    if let Some(p) = item.pointed_by.as_ref() {
                        self.elements.remove(p);
                    }
                }
            }
            IndexType::ForOp => {
                for item in Self::drain_by_query::<OpLenQuery>(&mut self.list, range) {
                    self.id_to_list_leaf.remove(&item.id.idlp());
                    if let Some(p) = item.pointed_by.as_ref() {
                        self.elements.remove(p);
                    }
                }
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

    fn mov(
        &mut self,
        from_index: usize,
        to_index: usize,
        elem_id: IdLp,
        new_pos_id: IdFull,
        kind: IndexType,
    ) {
        // PERF: can be optimized by inlining try update
        let item = ListItem {
            pointed_by: None,
            id: new_pos_id,
        };

        if cfg!(debug_assertions) {
            let item = self.get_list_item_at(from_index, kind).unwrap();
            assert_eq!(item.pointed_by, Some(elem_id.compact()));
        }

        self.list_insert(
            if to_index > from_index {
                to_index + 1
            } else {
                to_index
            },
            item,
            kind,
        );
        self.update_elem_pos(elem_id, new_pos_id.idlp(), false, true);
    }

    pub(crate) fn get(&self, index: usize, kind: IndexType) -> Option<&LoroValue> {
        if index >= self.len() {
            return None;
        }

        let item = self.get_list_item_at(index, kind).unwrap();
        let elem = item.pointed_by.unwrap();
        self.elements.get(&elem).map(|x| &x.value)
    }

    fn get_list_item_at(&self, index: usize, kind: IndexType) -> Option<&ListItem> {
        let cursor = match kind {
            IndexType::ForUser => self.list.query::<UserLenQuery>(&index)?,
            IndexType::ForOp => self.list.query::<OpLenQuery>(&index)?,
        };
        if !cursor.found {
            return None;
        }

        let item = self.list.get_elem(cursor.leaf())?;
        Some(item)
    }

    pub(crate) fn get_elem_id_at_given_pos(
        &self,
        index: usize,
        kind: IndexType,
    ) -> Option<CompactIdLp> {
        self.get_list_item_at(index, kind)
            .and_then(|x| x.pointed_by)
    }

    pub(crate) fn get_list_id_at(&self, index: usize, kind: IndexType) -> Option<ID> {
        self.get_list_item_at(index, kind).map(|x| x.id.id())
    }

    pub(crate) fn get_elem_id_at(&self, index: usize, kind: IndexType) -> Option<CompactIdLp> {
        self.get_list_item_at(index, kind)
            .and_then(|x| x.pointed_by)
    }

    pub(crate) fn convert_index(
        &self,
        index: usize,
        from: IndexType,
        to: IndexType,
    ) -> Option<usize> {
        let len = self.len_kind(from);
        if index == len {
            return Some(self.len_kind(to));
        }

        if index > len {
            return None;
        }

        let c = match from {
            IndexType::ForOp => self.list.query::<OpLenQuery>(&index).unwrap(),
            IndexType::ForUser => self.list.query::<UserLenQuery>(&index).unwrap(),
        };

        Some(self.get_user_index_of(c.cursor.leaf, to) as usize)
    }

    fn get_list_item(&self, id: IdLp) -> Option<&ListItem> {
        self.id_to_list_leaf
            .get(&id)
            .and_then(|leaf| self.list.get_elem(*leaf))
    }

    pub fn get_list_item_index(&self, id: IdLp) -> Option<usize> {
        self.id_to_list_leaf
            .get(&id)
            .map(|leaf| self.get_user_index_of(*leaf, IndexType::ForUser) as usize)
    }

    /// Get the user index of elem
    ///
    /// If we cannot find the list item in the list, we will return None.
    fn get_index_of_elem(&self, id: IdLp) -> Option<usize> {
        let elem = self.elements.get(&id.compact()).unwrap();
        self.get_list_item_index(elem.pos)
    }

    fn get_user_index_of(&self, leaf: LeafIndex, kind: IndexType) -> i32 {
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
        for (id, elem) in self.elements.iter() {
            let item = self
                .get_list_item(id.to_id())
                .expect("Elem's pos should be in the list");
            assert_eq!(item.pointed_by.unwrap(), *id);
        }

        for item in self.list.iter() {
            if let Some(elem_id) = item.pointed_by {
                let elem = self.elements.get(&elem_id).unwrap();
                assert_eq!(elem.pos, item.id.idlp());
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &LoroValue> {
        // PERF: can be optimized
        (0..self.len()).map(move |i| self.get(i, IndexType::ForUser).unwrap())
    }

    pub fn len(&self) -> usize {
        self.list.root_cache().user_len as usize
    }

    fn to_vec(&self) -> Vec<LoroValue> {
        self.iter().cloned().collect_vec()
    }

    /// push a new elem into the list
    fn push_inner(&mut self, list_item_id: IdFull, elem: Option<PushElemInfo>) {
        let pointed_by = elem.as_ref().map(|x| x.elem_id);
        if let Some(elem) = elem {
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

struct PushElemInfo {
    elem_id: CompactIdLp,
    value: LoroValue,
    last_set_id: IdLp,
}

impl ContainerState for MovableListState {
    fn container_idx(&self) -> ContainerIdx {
        self.idx
    }

    fn estimate_size(&self) -> usize {
        self.len() * 8
    }

    fn is_state_empty(&self) -> bool {
        self.list.is_empty() && self.elements.is_empty()
    }

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
        debug!("Received diff: {:?}", diff);

        let mut ans: Delta<Vec<ValueOrHandler>, ListDeltaMeta> = Delta::new();

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
                        self.list_insert_batch(
                            index,
                            insert.into_iter().map(|x| ListItem {
                                id: x,
                                pointed_by: None,
                            }),
                            IndexType::ForOp,
                        );
                        index += len;
                    }
                    DeltaItem::Delete {
                        delete,
                        attributes: _,
                    } => {
                        let user_index = self
                            .convert_index(index, IndexType::ForOp, IndexType::ForUser)
                            .unwrap();
                        let user_index_end = self
                            .convert_index(index + delete, IndexType::ForOp, IndexType::ForUser)
                            .unwrap();
                        ans = ans.compose(
                            Delta::new()
                                .retain(user_index)
                                .delete(user_index_end - user_index),
                        );
                        self.list_drain(index..index + delete, IndexType::ForOp);
                    }
                }
            }
        }

        {
            // apply element changes
            for delta_item in diff.elements.into_iter() {
                match delta_item {
                    crate::delta::ElementDelta::PosChange { id, new_pos } => {
                        let old_index = self.get_index_of_elem(id);
                        // don't need to update old list item, because it's handled by list diff already
                        let success = self.update_elem_pos(id, new_pos, true, false);

                        if success && old_index.is_some() {
                            let old_index = old_index.unwrap();
                            let new_index = self.get_index_of_elem(id).unwrap();
                            let new_delta = Delta::new().retain(new_index).retain_with_meta(
                                1,
                                ListDeltaMeta {
                                    move_from: Some(old_index),
                                },
                            );
                            ans = ans.compose(new_delta);
                        }
                    }
                    crate::delta::ElementDelta::ValueChange {
                        id,
                        new_value,
                        value_id,
                    } => {
                        let success = self.update_elem_value(id, new_value.clone(), value_id, true);
                        if success {
                            let index = self.get_index_of_elem(id);
                            if let Some(index) = index {
                                ans =
                                    ans.compose(Delta::new().retain(index).delete(1).insert(vec![
                                        ValueOrHandler::from_value(new_value, arena, txn, state),
                                    ]))
                            }
                        }
                    }
                    crate::delta::ElementDelta::New {
                        id,
                        new_pos,
                        new_value,
                        value_id,
                    } => {
                        self.create_new_elem(id, new_pos, new_value.clone(), value_id);
                        let index = self.get_index_of_elem(id).unwrap();
                        ans = ans.compose(Delta::new().retain(index).insert(vec![
                            ValueOrHandler::from_value(new_value, arena, txn, state),
                        ]))
                    }
                };
            }
        }

        Diff::List(ans)
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
                        self.elements.insert(
                            elem_id,
                            Element {
                                value: x.clone(),
                                value_id: elem_id.to_id(),
                                pos: pos_id.idlp(),
                            },
                        );

                        self.list_insert(
                            *pos + i,
                            ListItem {
                                id: pos_id,
                                pointed_by: Some(elem_id),
                            },
                            IndexType::ForOp,
                        );
                    }
                }
                _ => unreachable!(),
            },
            ListOp::Delete(span) => {
                self.list_drain(span.start() as usize..span.end() as usize, IndexType::ForOp);
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
                self.update_elem_value(*elem_id, value.clone(), op.idlp(), false);
            }
            ListOp::StyleStart { .. } | ListOp::StyleEnd => unreachable!(),
        }

        Ok(())
    }

    #[doc = r" Convert a state to a diff, such that an empty state will be transformed into the same as this state when it's applied."]
    fn to_diff(
        &mut self,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        Diff::List(
            Delta::new().insert(
                self.to_vec()
                    .into_iter()
                    .map(|v| ValueOrHandler::from_value(v, arena, txn, state))
                    .collect::<Vec<_>>(),
            ),
        )
    }

    fn get_value(&mut self) -> LoroValue {
        let list = self
            .list
            .iter_with_filter(|x| (x.user_len > 0, 0))
            .filter_map(|(_, item)| item.pointed_by.map(|eid| self.elements[&eid].value.clone()))
            .collect();
        LoroValue::List(Arc::new(list))
    }

    #[doc = r" Get the index of the child container"]
    #[allow(unused)]
    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        // FIXME: unimplemented
        None
    }

    #[allow(unused)]
    fn get_child_containers(&self) -> Vec<ContainerID> {
        // FIXME: unimplemented
        Vec::new()
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
        for item in self.list.iter() {
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
                let elem = self.elements.get(&elem_id).unwrap();
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

        for EncodedItem {
            invisible_list_item,
            pos_id_eq_elem_id,
        } in item_iter
        {
            // the first one don't need to read op, it only needs to read the invisible list items
            if !is_first {
                let last_set_op = last_set_op_iter.next().unwrap();
                let idlp = last_set_op.id_full().idlp();
                let mut get_pos_id_full = |elem_id: IdLp| {
                    let pos_id = if pos_id_eq_elem_id {
                        elem_id
                    } else {
                        let id = item_ids.next().unwrap();
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
                                self.push_inner(
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
                            self.push_inner(
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
                let id = item_ids.next().unwrap();
                let pos_id = IdLp::new(ctx.peers[id.peer_idx], id.lamport);
                let pos_o_id = ctx.oplog.idlp_to_id(pos_id).unwrap();
                let pos_id_full = IdFull {
                    peer: pos_id.peer,
                    lamport: pos_id.lamport,
                    counter: pos_o_id.counter,
                };
                self.push_inner(pos_id_full, None);
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
    use crate::{LoroDoc, ToJson};
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
