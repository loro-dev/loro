use std::{
    ops::RangeBounds,
    sync::{Arc, Mutex},
};

use crate::{container::ContainerID, event::Diff, LoroValue};
use fxhash::FxHashMap;
use generic_btree::{
    ArenaIndex, BTree, BTreeTrait, FindResult, LengthFinder, QueryResult, UseLengthFinder,
};

use super::ContainerState;

type ContainerMapping = Arc<Mutex<FxHashMap<ContainerID, ArenaIndex>>>;

#[derive(Clone)]
pub struct ListState {
    list: BTree<List>,
    child_container_to_leaf: Arc<Mutex<FxHashMap<ContainerID, ArenaIndex>>>,
}

struct List;
impl BTreeTrait for List {
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
        let diff = *cache - elements.len() as isize;
        *cache = elements.len() as isize;
        diff
    }

    fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
        *diff1 += diff2
    }
}

impl UseLengthFinder<List> for List {
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
    pub fn new() -> Self {
        let mut tree = BTree::new();
        let mapping: ContainerMapping = Arc::new(Mutex::new(Default::default()));
        let mapping_clone = mapping.clone();
        tree.set_listener(Some(Box::new(move |event| {
            if let LoroValue::Unresolved(container_id) = event.elem {
                let mut mapping = mapping_clone.lock().unwrap();
                if let Some(leaf) = event.target_leaf {
                    mapping.insert((**container_id).clone(), leaf);
                } else {
                    mapping.remove(container_id);
                }
            }
        })));

        Self {
            list: tree,
            child_container_to_leaf: mapping,
        }
    }

    pub fn get_child_container_index(&self, id: &ContainerID) -> Option<usize> {
        let mapping = self.child_container_to_leaf.lock().unwrap();
        let leaf = mapping.get(id)?;
        let node = self.list.get_node(*leaf);
        let elem_index = node
            .elements()
            .iter()
            .position(|x| x.as_unresolved().map(|x| &**x) == Some(id))?;
        let mut index = 0;
        self.list.visit_previous_caches(
            QueryResult {
                leaf: *leaf,
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
    }

    pub fn delete(&mut self, index: usize) -> Option<LoroValue> {
        self.list.delete::<LengthFinder>(&index)
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

        self.list.drain::<LengthFinder>(start..end);
    }

    pub fn insert_batch(&mut self, index: usize, values: impl IntoIterator<Item = LoroValue>) {
        let q = self.list.query::<LengthFinder>(&index);
        self.list.insert_many_by_query_result(&q, values);
    }

    pub fn len(&self) -> usize {
        *self.list.root_cache() as usize
    }
}

impl ContainerState for ListState {
    fn apply_diff(&mut self, diff: Diff) {
        if let Diff::List(delta) = diff {
            let mut index = 0;
            for span in delta {
                match span {
                    crate::delta::DeltaItem::Retain { len, .. } => {
                        index += len;
                    }
                    crate::delta::DeltaItem::Insert { value, .. } => {
                        let len = value.len();
                        self.insert_batch(index, value);
                        index += len;
                    }
                    crate::delta::DeltaItem::Delete { len, .. } => {
                        self.delete_range(index..index + len)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test() {
        let mut list = ListState::new();
        fn id(name: &str) -> ContainerID {
            ContainerID::new_root(name, crate::ContainerType::List)
        }
        list.insert(0, LoroValue::Unresolved(Box::new(id("abc"))));
        list.insert(0, LoroValue::Unresolved(Box::new(id("x"))));
        assert_eq!(list.get_child_container_index(&id("x")), Some(0));
        assert_eq!(list.get_child_container_index(&id("abc")), Some(1));
        list.insert(1, LoroValue::Bool(false));
        assert_eq!(list.get_child_container_index(&id("x")), Some(0));
        assert_eq!(list.get_child_container_index(&id("abc")), Some(2));
    }
}
