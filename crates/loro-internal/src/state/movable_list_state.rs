use std::sync::{Arc, Mutex, Weak};

use fxhash::FxHashMap;
use generic_btree::{BTree, Cursor, LeafIndex};
use loro_common::{CompactIdLp, ContainerID, IdFull, IdLp, LoroResult, LoroValue, ID};

use crate::{
    arena::SharedArena,
    container::idx::ContainerIdx,
    delta::DeltaItem,
    encoding::{StateSnapshotDecodeContext, StateSnapshotEncoder},
    event::{Diff, Index, InternalDiff},
    op::{Op, RawOp},
    txn::Transaction,
    DocState,
};

use self::list_item_tree::{MovableListTreeTrait, UserLenQuery};

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
            if self.pointed_by.is_some() {
                1
            } else {
                0
            }
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
        // This include the ones that were deleted
        pub all: i32,
        pub user: i32,
    }

    impl Add for Cache {
        type Output = Self;

        fn add(self, rhs: Self) -> Self::Output {
            Self {
                all: self.all + rhs.all,
                user: self.user + rhs.user,
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
                all: self.all - rhs.all,
                user: self.user - rhs.user,
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
                Cache { all: 1, user: 1 }
            } else {
                Cache { all: 1, user: 0 }
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
            cache.user as usize
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
            if elem.pointed_by.is_some() {
                (0, true)
            } else {
                (1, false)
            }
        }

        fn get_cache_entity_len(cache: &<MovableListTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.user as usize
        }
    }

    pub(crate) struct AllLenQueryT;
    pub(crate) type AllLenQuery = IndexQuery<AllLenQueryT, MovableListTreeTrait>;

    impl QueryByLen<MovableListTreeTrait> for AllLenQueryT {
        fn get_cache_len(cache: &<MovableListTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.all as usize
        }

        fn get_elem_len(elem: &<MovableListTreeTrait as BTreeTrait>::Elem) -> usize {
            1
        }

        fn get_offset_and_found(
            left: usize,
            elem: &<MovableListTreeTrait as BTreeTrait>::Elem,
        ) -> (usize, bool) {
            (0, true)
        }

        fn get_cache_entity_len(cache: &<MovableListTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.all as usize
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

    /// This update may not succeed if the given value_id is smaller than the existing value_id.
    fn try_update_elem_pos(&mut self, elem_id: IdLp, list_item_id: IdLp) {
        let id = elem_id.try_into().unwrap();
        let mut old_item_id = None;
        if let Some(element) = self.elements.get_mut(&id) {
            if element.pos > list_item_id {
                return;
            }

            old_item_id = Some(element.pos);
            // TODO: update list item pointed by
            element.pos = list_item_id;
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

        if let Some(old) = old_item_id {
            let leaf = self.id_to_list_leaf.get(&old).unwrap();
            self.list.update_leaf(*leaf, |elem| {
                elem.pointed_by = None;
                (true, None, None)
            });
        }
    }

    /// This update may not succeed if the given value_id is smaller than the existing value_id.
    fn try_update_elem_value(&mut self, elem: IdLp, value: LoroValue, value_id: IdLp) {
        let id = elem.try_into().unwrap();
        if let Some(element) = self.elements.get_mut(&id) {
            if element.value_id > value_id {
                return;
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
    }

    fn list_insert(&mut self, index: usize, item: ListItem) {
        let id = item.id;
        let (cursor, _) = self.list.insert::<UserLenQuery>(&index, item);
        self.id_to_list_leaf.insert(id.idlp(), cursor.leaf);
    }

    fn list_insert_batch(&mut self, index: usize, items: impl Iterator<Item = ListItem>) {
        for (i, item) in items.enumerate() {
            self.list_insert(index + i, item);
        }
    }

    fn list_drain(&mut self, range: std::ops::Range<usize>) {
        self.list.drain_by_query::<UserLenQuery>(range);
    }

    pub fn get(&self, index: usize) -> Option<&LoroValue> {
        if index >= self.len() {
            return None;
        }

        let item = self.get_list_item_at(index).unwrap();
        let elem = item.pointed_by.unwrap();
        self.elements.get(&elem).map(|x| &x.value)
    }

    fn get_list_item_at(&self, index: usize) -> Option<&ListItem> {
        let cursor = self.list.query::<UserLenQuery>(&index)?;
        if !cursor.found {
            return None;
        }

        let item = self.list.get_elem(cursor.leaf())?;
        Some(item)
    }

    pub(crate) fn get_id_at(&self, index: usize) -> Option<IdInfo> {
        self.get_list_item_at(index).map(|x| {
            let p = x.pointed_by.unwrap();
            let p: IdLp = p.to_id();
            if x.id.idlp() == p {
                IdInfo::Same(x.id.id())
            } else {
                IdInfo::Diff {
                    list_item_id: x.id.id(),
                    elem_id: p,
                }
            }
        })
    }

    pub fn get_list_item_index(&self, id: IdLp) -> Option<usize> {
        self.id_to_list_leaf.get(&id).map(|leaf| {
            let mut ans = 0;
            self.list.visit_previous_caches(
                Cursor {
                    leaf: *leaf,
                    offset: 0,
                },
                |cache| match cache {
                    generic_btree::PreviousCache::NodeCache(c) => ans += c.user,
                    generic_btree::PreviousCache::PrevSiblingElem(p) => {
                        ans += if p.pointed_by.is_some() { 1 } else { 0 }
                    }
                    generic_btree::PreviousCache::ThisElemAndOffset { .. } => {}
                },
            );

            ans as usize
        })
    }

    pub fn len(&self) -> usize {
        self.list.root_cache().user as usize
    }
}

pub(crate) enum IdInfo {
    Same(ID),
    Diff { list_item_id: ID, elem_id: IdLp },
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
                        );
                        index += len;
                    }
                    DeltaItem::Delete {
                        delete,
                        attributes: _,
                    } => {
                        self.list_drain(index..index + delete);
                    }
                }
            }
        }

        {
            // apply element changes
            for delta_item in diff.elements.into_iter() {
                match delta_item {
                    crate::delta::ElementDelta::PosChange { id, new_pos } => {
                        self.try_update_elem_pos(id, new_pos);
                    }
                    crate::delta::ElementDelta::ValueChange {
                        id,
                        new_value,
                        value_id,
                    } => self.try_update_elem_value(id, new_value, value_id),
                }
            }
        }

        todo!("Calculate diff")
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

    fn apply_local_op(&mut self, raw_op: &RawOp, op: &Op) -> LoroResult<()> {
        todo!()
    }

    #[doc = r" Convert a state to a diff, such that an empty state will be transformed into the same as this state when it's applied."]
    fn to_diff(
        &mut self,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        todo!()
    }

    fn get_value(&mut self) -> LoroValue {
        let list = self
            .list
            .iter_with_filter(|x| (x.user > 0, 0))
            .filter_map(|(_, item)| item.pointed_by.map(|eid| self.elements[&eid].value.clone()))
            .collect();
        LoroValue::List(Arc::new(list))
    }

    #[doc = r" Get the index of the child container"]
    #[allow(unused)]
    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        todo!()
    }

    #[allow(unused)]
    fn get_child_containers(&self) -> Vec<ContainerID> {
        todo!()
    }

    #[doc = r" Encode the ops and the blob that can be used to restore the state to the current state."]
    #[doc = r""]
    #[doc = r" State will use the provided encoder to encode the ops and export a blob."]
    #[doc = r" The ops should be encoded into the snapshot as well as the blob."]
    #[doc = r" The users then can use the ops and the blob to restore the state to the current state."]
    fn encode_snapshot(&self, encoder: StateSnapshotEncoder) -> Vec<u8> {
        todo!()
    }

    #[doc = r" Restore the state to the state represented by the ops and the blob that exported by `get_snapshot_ops`"]
    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext) {
        todo!()
    }
}
