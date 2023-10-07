use std::{
    ops::RangeBounds,
    sync::{Arc, Mutex},
};

use super::ContainerState;
use crate::{
    arena::SharedArena,
    container::{idx::ContainerIdx, ContainerID},
    delta::Delta,
    event::{Diff, Index},
    op::{Op, RawOp, RawOpContent},
    LoroValue,
};
use debug_log::debug_dbg;
use fxhash::FxHashMap;
use generic_btree::{
    iter,
    rle::{HasLength, Mergeable, Sliceable},
    ArenaIndex, BTree, BTreeTrait, Cursor, LeafIndex, LengthFinder, SplittedLeaves,
    UseLengthFinder,
};

type ContainerMapping = Arc<Mutex<FxHashMap<ContainerID, ArenaIndex>>>;

#[derive(Debug)]
pub struct ListState {
    idx: ContainerIdx,
    list: BTree<ListImpl>,
    in_txn: bool,
    undo_stack: Vec<UndoItem>,
    child_container_to_leaf: FxHashMap<ContainerID, LeafIndex>,
}

impl Clone for ListState {
    fn clone(&self) -> Self {
        Self {
            idx: self.idx,
            list: self.list.clone(),
            in_txn: false,
            undo_stack: Vec::new(),
            child_container_to_leaf: Default::default(),
        }
    }
}

#[derive(Debug)]
enum UndoItem {
    Insert { index: usize, len: usize },
    Delete { index: usize, value: LoroValue },
}

#[derive(Debug, Clone)]
struct Elem {
    vec: Vec<LoroValue>,
}

const MAX_LEN: usize = 16;
impl HasLength for Elem {
    fn rle_len(&self) -> usize {
        self.vec.len()
    }
}

impl Sliceable for Elem {
    fn _slice(&self, range: std::ops::Range<usize>) -> Self {
        Self {
            vec: self.vec[range].to_vec(),
        }
    }

    fn split(&mut self, pos: usize) -> Self {
        Self {
            vec: self.vec.split_off(pos),
        }
    }
}

impl Mergeable for Elem {
    fn can_merge(&self, rhs: &Self) -> bool {
        self.rle_len() + rhs.rle_len() < MAX_LEN
    }

    fn merge_right(&mut self, rhs: &Self) {
        self.vec.extend_from_slice(&rhs.vec);
    }

    fn merge_left(&mut self, left: &Self) {
        self.vec.splice(0..0, left.vec.iter().cloned());
    }
}

struct ListImpl;
impl BTreeTrait for ListImpl {
    type Elem = Elem;

    type Cache = isize;

    type CacheDiff = isize;

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
    fn get_elem_cache(elem: &Self::Elem) -> Self::Cache {
        elem.rle_len() as isize
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

// FIXME: update child_container_to_leaf
impl ListState {
    pub fn new(idx: ContainerIdx) -> Self {
        let mut tree = BTree::new();
        Self {
            idx,
            list: tree,
            in_txn: false,
            undo_stack: Vec::new(),
            child_container_to_leaf: Default::default(),
        }
    }

    pub fn get_child_container_index(&self, id: &ContainerID) -> Option<usize> {
        debug_dbg!(self.get_value());
        let leaf = *self.child_container_to_leaf.get(id).unwrap();
        let node = self.list.get_elem(leaf)?;
        let elem_index = node.vec.iter().position(|x| x.as_container() == Some(id))?;
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

        Some(index as usize + elem_index)
    }

    pub fn insert(&mut self, index: usize, value: LoroValue) {
        if self.list.is_empty() {
            let idx = self.list.push(Elem {
                vec: vec![value.clone()],
            });

            if value.is_container() {
                self.child_container_to_leaf
                    .insert(value.into_container().unwrap(), idx);
            }
            return;
        }

        let (leaf, data) =
            self.list
                .update_leaf_by_search::<LengthFinder>(&index, |elem, cursor| {
                    if elem.rle_len() < MAX_LEN {
                        elem.vec.insert(cursor.cursor.offset, value.clone());
                        Some((1, None, None))
                    } else {
                        Some((
                            1,
                            Some(Elem {
                                vec: vec![value.clone()],
                            }),
                            None,
                        ))
                    }
                });

        if value.is_container() {
            self.child_container_to_leaf
                .insert(value.into_container().unwrap(), leaf.unwrap().leaf);
        }

        for leaf in data.arr {
            for v in self.list.get_elem(leaf).unwrap().vec.iter() {
                if v.is_container() {
                    self.child_container_to_leaf
                        .insert(v.as_container().unwrap().clone(), leaf);
                }
            }
        }

        if self.in_txn {
            self.undo_stack.push(UndoItem::Insert { index, len: 1 });
        }
    }

    pub fn delete(&mut self, index: usize) {
        let mut value = None;
        self.list
            .update_leaf_by_search::<LengthFinder>(&index, |elem, cursor| {
                value = Some(elem.vec.remove(cursor.offset()));
                Some((-1, None, None))
            });
        if self.in_txn {
            self.undo_stack.push(UndoItem::Delete {
                index,
                value: value.unwrap(),
            });
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

        if self.in_txn {
            let self1 = &mut self.list;
            let q = start..end;
            let start1 = self1.query::<LengthFinder>(&q.start);
            let end1 = self1.query::<LengthFinder>(&q.end);
            for elem in iter::Drain::new(self1, start1, end1) {
                for value in elem.vec {
                    self.undo_stack.push(UndoItem::Delete {
                        index: start,
                        value,
                    })
                }
            }
        } else {
            let self1 = &mut self.list;
            let q = start..end;
            let start1 = self1.query::<LengthFinder>(&q.start);
            let end1 = self1.query::<LengthFinder>(&q.end);
            iter::Drain::new(self1, start1, end1);
        }
    }

    // PERF: use &[LoroValue]
    pub fn insert_batch(&mut self, index: usize, values: Vec<LoroValue>) {
        let (leaf, data) = if self.list.is_empty() {
            let leaf = self.list.push(Elem {
                vec: values.clone(),
            });
            (leaf, SplittedLeaves::default())
        } else {
            let (cursor, s) =
                self.list
                    .update_leaf_by_search::<LengthFinder>(&index, |elem, cursor| {
                        if elem.rle_len() + values.len() < MAX_LEN {
                            elem.vec
                                .splice(cursor.offset()..cursor.offset(), values.clone());
                            Some((values.len() as isize, None, None))
                        } else {
                            Some((
                                values.len() as isize,
                                Some(Elem {
                                    vec: values.clone(),
                                }),
                                None,
                            ))
                        }
                    });
            (cursor.unwrap().leaf, s)
        };

        for value in values {
            if let Ok(c) = value.into_container() {
                self.child_container_to_leaf.insert(c, leaf);
            }
        }

        for leaf in data.arr {
            for v in self.list.get_elem(leaf).unwrap().vec.iter() {
                if v.is_container() {
                    self.child_container_to_leaf
                        .insert(v.as_container().unwrap().clone(), leaf);
                }
            }
        }

        if self.in_txn {
            self.undo_stack.push(UndoItem::Insert { index, len: 1 });
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &LoroValue> {
        self.list.iter().map(|x| x.vec.iter()).flatten()
    }

    pub fn len(&self) -> usize {
        *self.list.root_cache() as usize
    }

    fn to_vec(&self) -> Vec<LoroValue> {
        let mut ans = Vec::with_capacity(self.len());
        for value in self.list.iter() {
            ans.extend_from_slice(&value.vec);
        }
        ans
    }

    pub fn get(&self, index: usize) -> Option<&LoroValue> {
        let result = self.list.query::<LengthFinder>(&index)?;
        if result.found {
            Some(&result.elem(&self.list).unwrap().vec[result.offset()])
        } else {
            None
        }
    }
}

impl ContainerState for ListState {
    fn apply_diff(&mut self, diff: &mut Diff, arena: &SharedArena) {
        match diff {
            Diff::List(delta) => {
                let mut index = 0;
                for span in delta.iter() {
                    match span {
                        crate::delta::DeltaItem::Retain { len, .. } => {
                            index += len;
                        }
                        crate::delta::DeltaItem::Insert { value, .. } => {
                            let len = value.len();
                            for value in value.iter() {
                                if value.is_container() {
                                    let c = value.as_container().unwrap();
                                    let idx = arena.register_container(c);
                                    arena.set_parent(idx, Some(self.idx));
                                }
                            }

                            self.insert_batch(index, value.clone());
                            index += len;
                        }
                        crate::delta::DeltaItem::Delete { len, .. } => {
                            self.delete_range(index..index + len)
                        }
                    }
                }
            }
            Diff::SeqRaw(delta) => {
                let mut index = 0;
                for span in delta.iter() {
                    match span {
                        crate::delta::DeltaItem::Retain { len, .. } => {
                            index += len;
                        }
                        crate::delta::DeltaItem::Insert { value, .. } => {
                            let mut arr = Vec::new();
                            for slices in value.0.iter() {
                                for i in slices.0.start..slices.0.end {
                                    let value = arena.get_value(i as usize).unwrap();
                                    if value.is_container() {
                                        let c = value.as_container().unwrap();
                                        let idx = arena.register_container(c);
                                        arena.set_parent(idx, Some(self.idx));
                                    }
                                    arr.push(value);
                                }
                            }
                            let len = arr.len();
                            self.insert_batch(index, arr);
                            index += len;
                        }
                        crate::delta::DeltaItem::Delete { len, .. } => {
                            self.delete_range(index..index + len)
                        }
                    }
                }
            }
            _ => unreachable!(),
        };
    }

    fn apply_op(&mut self, op: &RawOp, _: &Op, arena: &SharedArena) {
        match &op.content {
            RawOpContent::Map(_) => unreachable!(),
            RawOpContent::List(list) => match list {
                crate::container::list::list_op::ListOp::Insert { slice, pos } => match slice {
                    crate::container::text::text_content::ListSlice::RawData(list) => match list {
                        std::borrow::Cow::Borrowed(list) => {
                            for value in list.iter() {
                                if value.is_container() {
                                    let c = value.as_container().unwrap();
                                    let idx = arena.register_container(c);
                                    arena.set_parent(idx, Some(self.idx));
                                }
                            }
                            self.insert_batch(*pos, list.to_vec());
                        }
                        std::borrow::Cow::Owned(list) => {
                            for value in list.iter() {
                                if value.is_container() {
                                    let c = value.as_container().unwrap();
                                    let idx = arena.register_container(c);
                                    arena.set_parent(idx, Some(self.idx));
                                }
                            }
                            self.insert_batch(*pos, list.clone());
                        }
                    },
                    _ => unreachable!(),
                },
                crate::container::list::list_op::ListOp::Delete(del) => {
                    self.delete_range(del.pos as usize..del.pos as usize + del.len as usize);
                }
                crate::container::list::list_op::ListOp::StyleStart { .. } => unreachable!(),
                crate::container::list::list_op::ListOp::StyleEnd { .. } => unreachable!(),
            },
        }
    }

    #[doc = " Start a transaction"]
    #[doc = ""]
    #[doc = " The transaction may be aborted later, then all the ops during this transaction need to be undone."]
    fn start_txn(&mut self) {
        self.in_txn = true;
    }

    fn abort_txn(&mut self) {
        self.in_txn = false;
        while let Some(op) = self.undo_stack.pop() {
            match op {
                UndoItem::Insert { index, len } => {
                    self.delete_range(index..index + len);
                }
                UndoItem::Delete { index, value } => self.insert(index, value),
            }
        }
    }

    fn commit_txn(&mut self) {
        self.undo_stack.clear();
        self.in_txn = false;
    }

    fn get_value(&self) -> LoroValue {
        let ans = self.to_vec();
        LoroValue::List(Arc::new(ans))
    }

    #[doc = " Convert a state to a diff that when apply this diff on a empty state,"]
    #[doc = " the state will be the same as this state."]
    fn to_diff(&self) -> Diff {
        Diff::List(Delta::new().insert(self.to_vec()))
    }

    fn get_child_index(&self, id: &ContainerID) -> Option<Index> {
        self.get_child_container_index(id).map(Index::Seq)
    }

    fn get_child_containers(&self) -> Vec<ContainerID> {
        let mut ans = Vec::new();
        for elem in self.list.iter() {
            for value in elem.vec.iter() {
                if value.is_container() {
                    ans.push(value.as_container().unwrap().clone());
                }
            }
        }
        ans
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
        list.insert(0, LoroValue::Container(id("abc")));
        list.insert(0, LoroValue::Container(id("x")));
        assert_eq!(list.get_child_container_index(&id("x")), Some(0));
        assert_eq!(list.get_child_container_index(&id("abc")), Some(1));
        list.insert(1, LoroValue::Bool(false));
        assert_eq!(list.get_child_container_index(&id("x")), Some(0));
        assert_eq!(list.get_child_container_index(&id("abc")), Some(2));
    }
}
