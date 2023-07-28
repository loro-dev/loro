use std::{
    ops::RangeBounds,
    sync::{Arc, Mutex},
};

use crate::{
    container::{registry::ContainerIdx, ContainerID},
    delta::Delta,
    event::{Diff, Index},
    op::{RawOp, RawOpContent},
    refactor::arena::SharedArena,
    LoroValue,
};
use debug_log::debug_dbg;
use fxhash::FxHashMap;
use generic_btree::{
    ArenaIndex, BTree, BTreeTrait, FindResult, LengthFinder, QueryResult, UseLengthFinder,
};

use super::ContainerState;

type ContainerMapping = Arc<Mutex<FxHashMap<ContainerID, ArenaIndex>>>;

#[derive(Debug)]
pub struct ListState {
    idx: ContainerIdx,
    list: BTree<ListImpl>,
    in_txn: bool,
    undo_stack: Vec<UndoItem>,
    child_container_to_leaf: Arc<Mutex<FxHashMap<ContainerID, ArenaIndex>>>,
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

struct ListImpl;
impl BTreeTrait for ListImpl {
    type Elem = LoroValue;

    type Cache = isize;

    type CacheDiff = isize;

    const MAX_LEN: usize = 8;

    fn calc_cache_internal(
        cache: &mut Self::Cache,
        caches: &[generic_btree::Child<Self>],
        diff: Option<Self::CacheDiff>,
    ) -> Option<Self::CacheDiff> {
        match diff {
            Some(diff) => {
                *cache += diff;
                Some(diff)
            }
            None => {
                let mut new_cache = 0;
                for child in caches {
                    new_cache += child.cache;
                }

                let diff = new_cache - *cache;
                *cache = new_cache;
                Some(diff)
            }
        }
    }

    fn calc_cache_leaf(
        cache: &mut Self::Cache,
        elements: &[Self::Elem],
        _diff: Option<Self::CacheDiff>,
    ) -> Self::CacheDiff {
        let diff = elements.len() as isize - *cache;
        *cache = elements.len() as isize;
        diff
    }

    fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
        *diff1 += diff2
    }

    fn insert_batch(
        elements: &mut generic_btree::HeapVec<Self::Elem>,
        index: usize,
        _offset: usize,
        new_elements: impl IntoIterator<Item = Self::Elem>,
    ) where
        Self::Elem: Clone,
    {
        elements.splice(index..index, new_elements);
    }
}

impl UseLengthFinder<ListImpl> for ListImpl {
    fn get_len(cache: &isize) -> usize {
        *cache as usize
    }

    fn find_element_by_offset(elements: &[LoroValue], offset: usize) -> generic_btree::FindResult {
        if offset >= elements.len() {
            return FindResult::new_missing(elements.len(), offset - elements.len());
        }

        FindResult::new_found(offset, 0)
    }
}

impl ListState {
    pub fn new(idx: ContainerIdx) -> Self {
        let mut tree = BTree::new();
        let mapping: ContainerMapping = Arc::new(Mutex::new(Default::default()));
        let mapping_clone = mapping.clone();
        tree.set_listener(Some(Box::new(move |event| {
            if let LoroValue::Container(container_id) = event.elem {
                let mut mapping = mapping_clone.try_lock().unwrap();
                if let Some(leaf) = event.target_leaf {
                    mapping.insert((*container_id).clone(), leaf);
                } else {
                    mapping.remove(container_id);
                }
                drop(mapping);
            }
        })));

        Self {
            idx,
            list: tree,
            in_txn: false,
            undo_stack: Vec::new(),
            child_container_to_leaf: mapping,
        }
    }

    pub fn get_child_container_index(&self, id: &ContainerID) -> Option<usize> {
        debug_dbg!(self.get_value());
        let mapping = self.child_container_to_leaf.lock().unwrap();
        let leaf = *mapping.get(id)?;
        drop(mapping);
        let node = self.list.get_node_safe(leaf)?;
        let elem_index = node
            .elements()
            .iter()
            .position(|x| x.as_container() == Some(id))?;
        let mut index = 0;
        self.list.visit_previous_caches(
            QueryResult {
                leaf,
                elem_index: 0,
                offset: 0,
                found: true,
            },
            |cache| match cache {
                generic_btree::PreviousCache::NodeCache(cache) => {
                    index += *cache;
                }
                generic_btree::PreviousCache::PrevSiblingElem(..) => {
                    index += 1;
                }
                generic_btree::PreviousCache::ThisElemAndOffset { .. } => {}
            },
        );

        Some(index as usize + elem_index)
    }

    pub fn insert(&mut self, index: usize, value: LoroValue) {
        self.list.insert::<LengthFinder>(&index, value);
        if self.in_txn {
            self.undo_stack.push(UndoItem::Insert { index, len: 1 });
        }
    }

    pub fn delete(&mut self, index: usize) {
        let value = self.list.delete::<LengthFinder>(&index).unwrap();
        if self.in_txn {
            self.undo_stack.push(UndoItem::Delete { index, value });
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
            for value in self.list.drain::<LengthFinder>(start..end) {
                self.undo_stack.push(UndoItem::Delete {
                    index: start,
                    value,
                })
            }
        } else {
            self.list.drain::<LengthFinder>(start..end);
        }
    }

    // PERF: use &[LoroValue]
    pub fn insert_batch(&mut self, index: usize, values: Vec<LoroValue>) {
        let q = self.list.query::<LengthFinder>(&index);
        let old_len = self.len();
        self.list.insert_many_by_query_result(&q, values);
        if self.in_txn {
            let len = self.len() - old_len;
            self.undo_stack.push(UndoItem::Insert { index, len });
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &LoroValue> {
        self.list.iter()
    }

    pub fn len(&self) -> usize {
        *self.list.root_cache() as usize
    }

    fn to_vec(&self) -> Vec<LoroValue> {
        let mut ans = Vec::with_capacity(self.len());
        for value in self.list.iter() {
            ans.push(value.clone());
        }
        ans
    }

    pub fn get(&self, index: usize) -> Option<&LoroValue> {
        let result = self.list.query::<LengthFinder>(&index);
        if result.found {
            Some(result.elem(&self.list).unwrap())
        } else {
            None
        }
    }
}

impl ContainerState for ListState {
    fn apply_diff(&mut self, diff: &mut Diff, arena: &SharedArena) {
        debug_log::debug_log!("Apply List Diff {:#?}", diff);
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
                                    debug_dbg!(&value);
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
        debug_dbg!(&self.idx);
        debug_dbg!(&self.get_value());
    }

    fn apply_op(&mut self, op: RawOp, arena: &SharedArena) {
        match op.content {
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
                            self.insert_batch(pos, list.to_vec());
                        }
                        std::borrow::Cow::Owned(list) => {
                            for value in list.iter() {
                                if value.is_container() {
                                    let c = value.as_container().unwrap();
                                    let idx = arena.register_container(c);
                                    arena.set_parent(idx, Some(self.idx));
                                }
                            }
                            self.insert_batch(pos, list);
                        }
                    },
                    _ => unreachable!(),
                },
                crate::container::list::list_op::ListOp::Delete(del) => {
                    self.delete_range(del.pos as usize..del.pos as usize + del.len as usize);
                }
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
        for value in self.list.iter() {
            if value.is_container() {
                ans.push(value.as_container().unwrap().clone());
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
