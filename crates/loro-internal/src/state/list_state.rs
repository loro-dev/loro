use std::{
    ops::RangeBounds,
    sync::{Arc, Mutex, Weak},
};

use super::ContainerState;
use crate::{
    arena::SharedArena,
    container::{idx::ContainerIdx, list::list_op::ListOp, ContainerID},
    delta::Delta,
    encoding::{EncodeMode, StateSnapshotDecodeContext, StateSnapshotEncoder},
    event::{Diff, Index, InternalDiff},
    handler::ValueOrHandler,
    op::{ListSlice, Op, RawOp, RawOpContent},
    txn::Transaction,
    DocState, LoroValue,
};

use fxhash::FxHashMap;
use generic_btree::{
    iter,
    rle::{HasLength, Mergeable, Sliceable},
    BTree, BTreeTrait, Cursor, LeafIndex, LengthFinder, UseLengthFinder,
};
use loro_common::{IdFull, IdLpSpan, LoroResult};

#[derive(Debug)]
pub struct ListState {
    idx: ContainerIdx,
    list: BTree<ListImpl>,
    child_container_to_leaf: FxHashMap<ContainerID, LeafIndex>,
}

impl Clone for ListState {
    fn clone(&self) -> Self {
        Self {
            idx: self.idx,
            list: self.list.clone(),
            child_container_to_leaf: Default::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Elem {
    pub v: LoroValue,
    pub id: IdFull,
}

impl HasLength for Elem {
    fn rle_len(&self) -> usize {
        1
    }
}

impl Sliceable for Elem {
    fn _slice(&self, range: std::ops::Range<usize>) -> Self {
        assert_eq!(range.start, 0);
        assert_eq!(range.end, 1);
        self.clone()
    }

    fn split(&mut self, _pos: usize) -> Self {
        unreachable!()
    }
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

struct ListImpl;
impl BTreeTrait for ListImpl {
    type Elem = Elem;
    type Cache = isize;
    type CacheDiff = isize;
    const USE_DIFF: bool = true;

    #[inline(always)]
    fn calc_cache_internal(
        cache: &mut Self::Cache,
        caches: &[generic_btree::Child<Self>],
    ) -> Self::CacheDiff {
        let mut new_cache = 0;
        for child in caches {
            new_cache += child.cache;
        }

        let diff = new_cache - *cache;
        *cache = new_cache;
        diff
    }

    #[inline(always)]
    fn apply_cache_diff(cache: &mut Self::Cache, diff: &Self::CacheDiff) {
        *cache += diff;
    }

    #[inline(always)]
    fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
        *diff1 += diff2
    }

    #[inline(always)]
    fn get_elem_cache(_elem: &Self::Elem) -> Self::Cache {
        1
    }

    #[inline(always)]
    fn new_cache_to_diff(cache: &Self::Cache) -> Self::CacheDiff {
        *cache
    }

    fn sub_cache(cache_lhs: &Self::Cache, cache_rhs: &Self::Cache) -> Self::CacheDiff {
        cache_lhs - cache_rhs
    }
}

impl UseLengthFinder<ListImpl> for ListImpl {
    fn get_len(cache: &isize) -> usize {
        *cache as usize
    }
}

impl ListState {
    pub fn new(idx: ContainerIdx) -> Self {
        let tree = BTree::new();
        Self {
            idx,
            list: tree,
            child_container_to_leaf: Default::default(),
        }
    }

    pub fn get_child_container_index(&self, id: &ContainerID) -> Option<usize> {
        let leaf = *self.child_container_to_leaf.get(id)?;
        self.list.get_elem(leaf)?;
        let mut index = 0;
        self.list
            .visit_previous_caches(Cursor { leaf, offset: 0 }, |cache| match cache {
                generic_btree::PreviousCache::NodeCache(cache) => {
                    index += *cache;
                }
                generic_btree::PreviousCache::PrevSiblingElem(..) => {
                    index += 1;
                }
                generic_btree::PreviousCache::ThisElemAndOffset { .. } => {}
            });

        Some(index as usize)
    }

    pub fn insert(&mut self, index: usize, value: LoroValue, id: IdFull) {
        if index > self.len() {
            panic!("Index {index} out of range. The length is {}", self.len());
        }

        if self.list.is_empty() {
            let idx = self.list.push(Elem {
                v: value.clone(),
                id,
            });

            if value.is_container() {
                self.child_container_to_leaf
                    .insert(value.into_container().unwrap(), idx.leaf);
            }
            return;
        }

        let (leaf, data) = self.list.insert::<LengthFinder>(
            &index,
            Elem {
                v: value.clone(),
                id,
            },
        );

        if value.is_container() {
            self.child_container_to_leaf
                .insert(value.into_container().unwrap(), leaf.leaf);
        }

        for leaf in data.arr {
            let v = &self.list.get_elem(leaf).unwrap().v;
            if v.is_container() {
                self.child_container_to_leaf
                    .insert(v.as_container().unwrap().clone(), leaf);
            }
        }
    }

    pub fn delete(&mut self, index: usize) {
        let leaf = self.list.query::<LengthFinder>(&index);
        let leaf = self.list.remove_leaf(leaf.unwrap().cursor).unwrap();
        if leaf.v.is_container() {
            self.child_container_to_leaf
                .remove(leaf.v.as_container().unwrap());
        }
    }

    pub fn delete_range(&mut self, range: impl RangeBounds<usize>) {
        let start: usize = match range.start_bound() {
            std::ops::Bound::Included(x) => *x,
            std::ops::Bound::Excluded(x) => *x + 1,
            std::ops::Bound::Unbounded => 0,
        };
        let end: usize = match range.end_bound() {
            std::ops::Bound::Included(x) => *x + 1,
            std::ops::Bound::Excluded(x) => *x,
            std::ops::Bound::Unbounded => self.len(),
        };
        if end - start == 1 {
            self.delete(start);
            return;
        }

        let list = &mut self.list;
        let q = start..end;
        let start1 = list.query::<LengthFinder>(&q.start);
        let end1 = list.query::<LengthFinder>(&q.end);
        for v in iter::Drain::new(list, start1, end1) {
            if v.v.is_container() {
                self.child_container_to_leaf
                    .remove(v.v.as_container().unwrap());
            }
        }
    }

    // PERF: use &[LoroValue]
    // PERF: batch
    pub fn insert_batch(&mut self, index: usize, values: Vec<LoroValue>, start_id: IdFull) {
        let mut id = start_id;
        for (i, value) in values.into_iter().enumerate() {
            self.insert(index + i, value, id);
            id = id.inc(1);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &LoroValue> {
        self.list.iter().map(|x| &x.v)
    }

    #[allow(unused)]
    pub(crate) fn iter_with_id(&self) -> impl Iterator<Item = &Elem> {
        self.list.iter()
    }

    pub fn len(&self) -> usize {
        *self.list.root_cache() as usize
    }

    fn to_vec(&self) -> Vec<LoroValue> {
        let mut ans = Vec::with_capacity(self.len());
        for value in self.list.iter() {
            ans.push(value.v.clone());
        }
        ans
    }

    pub fn get(&self, index: usize) -> Option<&LoroValue> {
        let result = self.list.query::<LengthFinder>(&index)?;
        if result.found {
            Some(&result.elem(&self.list).unwrap().v)
        } else {
            None
        }
    }

    pub fn get_id_at(&self, index: usize) -> Option<IdFull> {
        let result = self.list.query::<LengthFinder>(&index)?;
        if result.found {
            Some(result.elem(&self.list).unwrap().id)
        } else {
            None
        }
    }

    #[allow(unused)]
    pub(crate) fn check(&self) {
        for value in self.iter() {
            if let LoroValue::Container(c) = value {
                self.get_child_index(c).unwrap();
            }
        }
    }
}

impl ContainerState for ListState {
    fn container_idx(&self) -> ContainerIdx {
        self.idx
    }

    fn estimate_size(&self) -> usize {
        // TODO: this is inaccurate
        self.list.node_len() * std::mem::size_of::<isize>()
            + self.len() * std::mem::size_of::<Elem>()
            + self.child_container_to_leaf.len() * std::mem::size_of::<(ContainerID, LeafIndex)>()
    }

    fn is_state_empty(&self) -> bool {
        self.list.is_empty()
    }

    fn apply_diff_and_convert(
        &mut self,
        diff: InternalDiff,
        arena: &SharedArena,
        txn: &Weak<Mutex<Option<Transaction>>>,
        state: &Weak<Mutex<DocState>>,
    ) -> Diff {
        let InternalDiff::ListRaw(delta) = diff else {
            unreachable!()
        };
        let mut ans: Delta<_, _> = Delta::default();
        let mut index = 0;
        for span in delta.iter() {
            match span {
                crate::delta::DeltaItem::Retain { retain: len, .. } => {
                    index += len;
                    ans = ans.retain(*len);
                }
                crate::delta::DeltaItem::Insert { insert: value, .. } => {
                    let mut arr = Vec::new();
                    for slices in value.ranges.iter() {
                        for i in slices.0.start..slices.0.end {
                            let value = arena.get_value(i as usize).unwrap();
                            arr.push(value);
                        }
                    }
                    ans = ans.insert(
                        arr.iter()
                            .map(|v| ValueOrHandler::from_value(v.clone(), arena, txn, state))
                            .collect::<Vec<_>>(),
                    );
                    let len = arr.len();
                    self.insert_batch(index, arr, value.id);
                    index += len;
                }
                crate::delta::DeltaItem::Delete { delete: len, .. } => {
                    self.delete_range(index..index + len);
                    ans = ans.delete(*len);
                }
            }
        }

        Diff::List(ans)
    }

    fn apply_diff(
        &mut self,
        diff: InternalDiff,
        arena: &SharedArena,
        _txn: &Weak<Mutex<Option<Transaction>>>,
        _state: &Weak<Mutex<DocState>>,
    ) {
        match diff {
            InternalDiff::ListRaw(delta) => {
                let mut index = 0;
                for span in delta.iter() {
                    match span {
                        crate::delta::DeltaItem::Retain { retain: len, .. } => {
                            index += len;
                        }
                        crate::delta::DeltaItem::Insert { insert: value, .. } => {
                            let mut arr = Vec::new();
                            for slices in value.ranges.iter() {
                                for i in slices.0.start..slices.0.end {
                                    let value = arena.get_value(i as usize).unwrap();
                                    arr.push(value);
                                }
                            }
                            let len = arr.len();

                            self.insert_batch(index, arr, value.id);
                            index += len;
                        }
                        crate::delta::DeltaItem::Delete { delete: len, .. } => {
                            self.delete_range(index..index + len);
                        }
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    fn apply_local_op(&mut self, op: &RawOp, _: &Op) -> LoroResult<()> {
        match &op.content {
            RawOpContent::Map(_) => unreachable!(),
            RawOpContent::Tree(_) => unreachable!(),
            RawOpContent::List(list) => match list {
                ListOp::Insert { slice, pos } => match slice {
                    ListSlice::RawData(list) => match list {
                        std::borrow::Cow::Borrowed(list) => {
                            self.insert_batch(*pos, list.to_vec(), op.id_full());
                        }
                        std::borrow::Cow::Owned(list) => {
                            self.insert_batch(*pos, list.clone(), op.id_full());
                        }
                    },
                    _ => unreachable!(),
                },
                ListOp::Delete(del) => {
                    self.delete_range(del.span.to_urange());
                }
                ListOp::Move { .. } => {
                    todo!("invoke move")
                }
                ListOp::StyleStart { .. } => unreachable!(),
                ListOp::StyleEnd { .. } => unreachable!(),
                ListOp::DeleteMovableListItem { .. } => {
                    unreachable!()
                }
            },
        }
        Ok(())
    }

    #[doc = " Convert a state to a diff that when apply this diff on a empty state,"]
    #[doc = " the state will be the same as this state."]
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
        let ans = self.to_vec();
        LoroValue::List(Arc::new(ans))
    }

    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        self.get_child_container_index(id).map(Index::Seq)
    }

    fn get_child_containers(&self) -> Vec<ContainerID> {
        let mut ans = Vec::new();
        for elem in self.list.iter() {
            if elem.v.is_container() {
                ans.push(elem.v.as_container().unwrap().clone());
            }
        }
        ans
    }

    #[doc = "Get a list of ops that can be used to restore the state to the current state"]
    fn encode_snapshot(&self, mut encoder: StateSnapshotEncoder) -> Vec<u8> {
        for elem in self.list.iter() {
            let id_span: IdLpSpan = elem.id.idlp().into();
            encoder.encode_op(id_span, || unimplemented!());
        }

        Vec::new()
    }

    #[doc = "Restore the state to the state represented by the ops that exported by `get_snapshot_ops`"]
    fn import_from_snapshot_ops(&mut self, ctx: StateSnapshotDecodeContext) {
        assert_eq!(ctx.mode, EncodeMode::Snapshot);
        let mut index = 0;
        for op in ctx.ops {
            let value = op.op.content.as_list().unwrap().as_insert().unwrap().0;
            let list = ctx
                .oplog
                .arena
                .get_values(value.0.start as usize..value.0.end as usize);
            let len = list.len();
            self.insert_batch(index, list, op.id_full());
            index += len;
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test() {
        let mut list = ListState::new(ContainerIdx::from_index_and_type(
            0,
            loro_common::ContainerType::List,
        ));
        fn id(name: &str) -> ContainerID {
            ContainerID::new_root(name, crate::ContainerType::List)
        }
        list.insert(0, LoroValue::Container(id("abc")), IdFull::new(0, 0, 0));
        list.insert(0, LoroValue::Container(id("x")), IdFull::new(0, 0, 0));
        assert_eq!(list.get_child_container_index(&id("x")), Some(0));
        assert_eq!(list.get_child_container_index(&id("abc")), Some(1));
        list.insert(1, LoroValue::Bool(false), IdFull::new(0, 0, 0));
        assert_eq!(list.get_child_container_index(&id("x")), Some(0));
        assert_eq!(list.get_child_container_index(&id("abc")), Some(2));
    }
}
