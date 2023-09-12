//! This map a Range<usize> to a set of style
//!

use std::{ops::Range, sync::Arc, usize};

use fxhash::{FxHashMap, FxHashSet};
use generic_btree::{
    rle::{
        delete_range_in_elements, scan_and_merge, update_slice, HasLength, Mergeable, Sliceable,
    },
    BTree, BTreeTrait, LengthFinder, UseLengthFinder,
};
use loro_common::{ContainerID, LoroValue};

use crate::InternalString;

use super::{Style, StyleInner};

/// This struct keep the mapping of ranges to numbers
///
/// It's initialized with usize::MAX/2 length.
#[derive(Debug, Clone)]
pub(super) struct StyleRangeMap(BTree<RangeNumMapTrait>);

#[derive(Debug, Clone)]
struct RangeNumMapTrait;

#[derive(Debug, Clone)]
struct Elem {
    styles: FxHashMap<InternalString, StyleValue>,
    len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum StyleValue {
    One(Arc<StyleInner>),
    Set(FxHashSet<Arc<StyleInner>>),
}

impl Default for StyleRangeMap {
    fn default() -> Self {
        Self::new()
    }
}

impl StyleValue {
    pub fn to_styles(&self) -> Vec<Style> {
        match self {
            StyleValue::One(x) => {
                vec![x.to_style()]
            }
            StyleValue::Set(set) => set.iter().map(|x| x.to_style()).collect(),
        }
    }
}

impl StyleRangeMap {
    pub fn new() -> Self {
        let mut tree = BTree::new();
        tree.insert_by_query_result(
            tree.first_full_path(),
            Elem {
                styles: Default::default(),
                len: usize::MAX / 4,
            },
        );

        Self(tree)
    }

    pub fn annotate(&mut self, range: Range<usize>, style: Arc<StyleInner>) {
        let range = self.0.range::<LengthFinder>(range);
        self.0.update(&range.start..&range.end, &mut |mut slice| {
            let ans = update_slice::<Elem, _>(&mut slice, &mut |x| {
                if style.info.mergeable() {
                    // only leave one value with the greatest lamport if the style is mergeable
                    if let Some(StyleValue::One(old)) = x.styles.get_mut(&style.key) {
                        if **old < *style {
                            *old = style.clone();
                        }
                    } else {
                        x.styles
                            .insert(style.key.clone(), StyleValue::One(style.clone()));
                    }
                } else {
                    // styles can coexist
                    let StyleValue::Set(set) = x
                        .styles
                        .entry(style.key.clone())
                        .or_insert_with(|| StyleValue::Set(FxHashSet::default()))
                    else {
                        unreachable!()
                    };
                    set.insert(style.clone());
                }

                false
            });

            scan_and_merge(slice.elements, slice.start.map(|x| x.0).unwrap_or(0));
            (ans, None)
        });
    }

    pub fn insert(&mut self, pos: usize, len: usize) {
        let range = self.0.range::<LengthFinder>(pos..pos + 1);
        let mut done = false;
        self.0.update(&range.start..&range.end, &mut |slice| {
            if done {
                return (false, None);
            }

            let index = slice.start.map(|x| x.0).unwrap_or(0);
            slice.elements[index].len += len;
            done = true;
            (true, Some(len as isize))
        })
    }

    pub fn get(&mut self, index: usize) -> Option<&FxHashMap<InternalString, StyleValue>> {
        let result = self.0.query::<LengthFinder>(&index);
        self.0.get_elem(&result).map(|x| &x.styles)
    }

    pub fn iter(
        &self,
    ) -> impl Iterator<Item = (Range<usize>, &FxHashMap<InternalString, StyleValue>)> + '_ {
        let mut index = 0;
        self.0.iter().filter_map(move |elem| {
            let len = elem.len;
            let value = &elem.styles;
            let range = index..index + len;
            index += len;
            if elem.styles.is_empty() {
                return None;
            }

            Some((range, value))
        })
    }

    pub fn delete(&mut self, range: Range<usize>) {
        self.0.drain::<LengthFinder>(range);
    }

    pub fn len(&self) -> usize {
        *self.0.root_cache()
    }
}

impl UseLengthFinder<RangeNumMapTrait> for RangeNumMapTrait {
    fn get_len(cache: &usize) -> usize {
        *cache
    }

    fn find_element_by_offset(elements: &[Elem], offset: usize) -> generic_btree::FindResult {
        let mut left = offset;
        for (i, elem) in elements.iter().enumerate() {
            if left >= elem.len {
                left -= elem.len;
            } else {
                return generic_btree::FindResult::new_found(i, left);
            }
        }

        generic_btree::FindResult::new_missing(elements.len(), left)
    }

    #[inline]
    fn finder_drain_range(
        elements: &mut generic_btree::HeapVec<<RangeNumMapTrait as BTreeTrait>::Elem>,
        start: Option<generic_btree::QueryResult>,
        end: Option<generic_btree::QueryResult>,
    ) -> Box<dyn Iterator<Item = Elem> + '_> {
        Box::new(delete_range_in_elements(elements, start, end).into_iter())
    }

    fn finder_delete_range(
        elements: &mut generic_btree::HeapVec<<RangeNumMapTrait as BTreeTrait>::Elem>,
        start: Option<generic_btree::QueryResult>,
        end: Option<generic_btree::QueryResult>,
    ) {
        delete_range_in_elements(elements, start, end);
    }
}

impl HasLength for Elem {
    fn rle_len(&self) -> usize {
        self.len
    }
}

impl Mergeable for Elem {
    fn can_merge(&self, rhs: &Self) -> bool {
        self.styles == rhs.styles || rhs.len == 0
    }

    fn merge_right(&mut self, rhs: &Self) {
        self.len += rhs.len
    }

    fn merge_left(&mut self, left: &Self) {
        self.len += left.len;
    }
}

impl Sliceable for Elem {
    fn slice(&self, range: impl std::ops::RangeBounds<usize>) -> Self {
        let len = match range.end_bound() {
            std::ops::Bound::Included(x) => x + 1,
            std::ops::Bound::Excluded(x) => *x,
            std::ops::Bound::Unbounded => self.len,
        } - match range.start_bound() {
            std::ops::Bound::Included(x) => *x,
            std::ops::Bound::Excluded(x) => x + 1,
            std::ops::Bound::Unbounded => 0,
        };
        Elem {
            styles: self.styles.clone(),
            len,
        }
    }

    fn slice_(&mut self, range: impl std::ops::RangeBounds<usize>)
    where
        Self: Sized,
    {
        let len = match range.end_bound() {
            std::ops::Bound::Included(x) => x + 1,
            std::ops::Bound::Excluded(x) => *x,
            std::ops::Bound::Unbounded => self.len,
        } - match range.start_bound() {
            std::ops::Bound::Included(x) => *x,
            std::ops::Bound::Excluded(x) => x + 1,
            std::ops::Bound::Unbounded => 0,
        };

        self.len = len;
    }
}

impl BTreeTrait for RangeNumMapTrait {
    type Elem = Elem;
    type Cache = usize;
    type CacheDiff = isize;

    const MAX_LEN: usize = 8;

    fn calc_cache_internal(
        cache: &mut Self::Cache,
        caches: &[generic_btree::Child<Self>],
        diff: Option<isize>,
    ) -> Option<isize> {
        match diff {
            Some(diff) => {
                *cache = (*cache as isize + diff) as usize;
                Some(diff)
            }
            None => {
                let new_cache = caches.iter().map(|c| c.cache).sum();
                let diff = new_cache as isize - *cache as isize;
                *cache = new_cache;
                Some(diff)
            }
        }
    }

    fn calc_cache_leaf(
        cache: &mut Self::Cache,
        elements: &[Self::Elem],
        _: Option<Self::CacheDiff>,
    ) -> isize {
        let new_cache = elements.iter().map(|c| c.len).sum();
        let diff = new_cache as isize - *cache as isize;
        *cache = new_cache;
        diff
    }

    fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
        *diff1 += diff2;
    }
}

#[cfg(test)]
mod test {
    use loro_common::PeerID;

    use crate::{change::Lamport, container::richtext::TextStyleInfo};

    use super::*;

    fn new_style(n: i32) -> Arc<StyleInner> {
        Arc::new(StyleInner {
            lamport: n as Lamport,
            peer: n as PeerID,
            cnt: n,
            key: n.to_string().into(),
            info: TextStyleInfo::default(),
        })
    }

    #[test]
    fn test_basic_insert() {
        let mut map = StyleRangeMap::default();
        map.annotate(1..10, new_style(1));
        {
            map.insert(0, 1);
            assert_eq!(map.iter().count(), 1);
            for (range, map) in map.iter() {
                assert_eq!(range, 2..11);
                assert_eq!(map.len(), 1);
            }
        }
        {
            map.insert(11, 1);
            assert_eq!(map.iter().count(), 1);
            for (range, map) in map.iter() {
                assert_eq!(range, 2..11);
                assert_eq!(map.len(), 1);
            }
        }
        {
            map.insert(10, 1);
            assert_eq!(map.iter().count(), 1);
            for (range, map) in map.iter() {
                assert_eq!(range, 2..12);
                assert_eq!(map.len(), 1);
            }
        }
    }

    #[test]
    fn delete_style() {
        let mut map = StyleRangeMap::default();
        map.annotate(1..10, new_style(1));
        {
            map.delete(0..2);
            assert_eq!(map.iter().count(), 1);
            for (range, map) in map.iter() {
                assert_eq!(range, 0..8);
                assert_eq!(map.len(), 1);
            }
        }
        {
            map.delete(2..4);
            for (range, map) in map.iter() {
                assert_eq!(range, 0..6);
                assert_eq!(map.len(), 1);
            }
            assert_eq!(map.iter().count(), 1);
        }
        {
            map.delete(6..8);
            assert_eq!(map.iter().count(), 1);
            for (range, map) in map.iter() {
                assert_eq!(range, 0..6);
                assert_eq!(map.len(), 1);
            }
        }
    }
}
