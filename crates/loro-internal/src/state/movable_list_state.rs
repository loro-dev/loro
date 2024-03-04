use std::sync::{Arc, Mutex, Weak};

use fxhash::FxHashMap;
use generic_btree::{BTree, Cursor, LeafIndex};
use loro_common::{CompactIdLp, ContainerID, IdFull, IdLp, LoroResult, LoroValue, ID};

use crate::{
    arena::SharedArena,
    container::{
        idx::ContainerIdx,
        list::list_op::{InnerListOp, ListOp},
    },
    delta::DeltaItem,
    encoding::{StateSnapshotDecodeContext, StateSnapshotEncoder},
    event::{Diff, Index, InternalDiff},
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
    /// This includes the deleted ones.
    ForUser,
    ForOp,
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
            if elem.pointed_by.is_some() {
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

    /// This update may not succeed if the given value_id is smaller than the existing value_id.
    #[must_use]
    fn try_update_elem_pos(&mut self, elem_id: IdLp, list_item_id: IdLp) -> bool {
        let id = elem_id.try_into().unwrap();
        let mut old_item_id = None;
        if let Some(element) = self.elements.get_mut(&id) {
            if element.pos > list_item_id {
                return false;
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

        true
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

    fn list_insert(&mut self, index: usize, item: ListItem, kind: IndexType) {
        let id = item.id;
        let (cursor, _) = match kind {
            IndexType::ForUser => self.list.insert::<UserLenQuery>(&index, item),
            IndexType::ForOp => self.list.insert::<OpLenQuery>(&index, item),
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

    fn list_drain(&mut self, range: std::ops::Range<usize>, kind: IndexType) {
        match kind {
            IndexType::ForUser => {
                for item in self.list.drain_by_query::<UserLenQuery>(range) {
                    if let Some(p) = item.pointed_by.as_ref() {
                        self.elements.remove(p);
                    }
                }
            }
            IndexType::ForOp => {
                for item in self.list.drain_by_query::<OpLenQuery>(range) {
                    if let Some(p) = item.pointed_by.as_ref() {
                        self.elements.remove(p);
                    }
                }
            }
        }
    }

    fn mov(
        &mut self,
        from_index: usize,
        to_index: usize,
        elem_id: IdLp,
        new_pos_id: IdFull,
        kind: IndexType,
    ) {
        if self.try_update_elem_pos(elem_id, new_pos_id.idlp()) {
            let item = ListItem {
                pointed_by: Some(elem_id.compact()),
                id: new_pos_id,
            };

            if cfg!(debug_assertions) {
                let item = self.get_list_item_at(from_index, kind).unwrap();
                assert_eq!(item.pointed_by, Some(elem_id.compact()));
            }

            match kind {
                IndexType::ForUser => self.list.insert::<UserLenQuery>(&to_index, item),
                IndexType::ForOp => self.list.insert::<OpLenQuery>(&to_index, item),
            };
        }
    }

    pub fn get(&self, index: usize, kind: IndexType) -> Option<&LoroValue> {
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

    pub(crate) fn get_id_at(&self, index: usize, kind: IndexType) -> Option<IdInfo> {
        self.get_list_item_at(index, kind).map(|x| {
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

    pub fn convert_user_index_to_op_index(&self, index: usize) -> Option<usize> {
        if index == self.len() {
            return Some(self.list.root_cache().include_dead_len as usize);
        }

        if index > self.len() {
            return None;
        }

        let c = self.list.query::<UserLenQuery>(&index).unwrap();
        Some(self.get_user_index_of(c.cursor.leaf, IndexType::ForOp) as usize)
    }

    pub fn get_list_item_index(&self, id: IdLp) -> Option<usize> {
        self.id_to_list_leaf
            .get(&id)
            .map(|leaf| self.get_user_index_of(*leaf, IndexType::ForUser) as usize)
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

    pub fn iter(&self) -> impl Iterator<Item = &LoroValue> {
        (0..self.len()).map(move |i| self.get(i, IndexType::ForUser).unwrap())
    }

    pub fn len(&self) -> usize {
        self.list.root_cache().user_len as usize
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
                            IndexType::ForOp,
                        );
                        index += len;
                    }
                    DeltaItem::Delete {
                        delete,
                        attributes: _,
                    } => {
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

    fn apply_local_op(&mut self, op: &RawOp, _: &Op) -> LoroResult<()> {
        match op.content.as_list().unwrap() {
            ListOp::Insert { slice, pos } => match slice {
                ListSlice::RawData(list) => {
                    let mut a = None;
                    let mut b = None;
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
            ListOp::DeleteMovableListItem {
                list_item_id,
                elem_id,
                pos,
            } => {
                self.list_drain(*pos..*pos + 1, IndexType::ForOp);
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
            ListOp::StyleStart { .. } | ListOp::StyleEnd => todo!(),
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
        todo!()
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
