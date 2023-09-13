//! This map a Range<usize> to a set of style
//!

use std::{collections::BTreeSet, ops::Range, sync::Arc, usize};

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

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub(super) struct StyleValue {
    set: BTreeSet<Arc<StyleInner>>,
    should_merge: bool,
}

impl Default for StyleRangeMap {
    fn default() -> Self {
        Self::new()
    }
}

impl StyleValue {
    pub fn new(mergeable: bool) -> Self {
        Self {
            set: Default::default(),
            should_merge: mergeable,
        }
    }

    pub fn to_styles(&self) -> Box<dyn Iterator<Item = Style> + '_> {
        if self.should_merge {
            Box::new(self.set.iter().rev().take(1).filter_map(|x| x.to_style()))
        } else {
            Box::new(self.set.iter().filter_map(|x| x.to_style()))
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
                // only leave one value with the greatest lamport if the style is mergeable
                if let Some(set) = x.styles.get_mut(&style.key) {
                    set.set.insert(style.clone());
                    // TODO: Doc this, and validate it earlier
                    assert_eq!(
                        set.should_merge,
                        style.info.mergeable(),
                        "Merge behavior should be the same for the same style key"
                    );
                } else {
                    let mut style_set = StyleValue::new(style.info.mergeable());
                    style_set.set.insert(style.clone());
                    x.styles.insert(style.key.clone(), style_set);
                }
                false
            });

            scan_and_merge(slice.elements, slice.start.map(|x| x.0).unwrap_or(0));
            (ans, None)
        });
    }

    /// Insert entities at `pos` with length of `len`
    ///
    /// # Internal
    ///
    /// When inserting new text, we need to calculate the StyleSet of the new text based on the StyleSet before and after the insertion position.
    /// (It should be the intersection of the StyleSet before and after). The proof is as follows:
    ///
    /// Suppose when inserting text at position pos, the style set at positions pos - 1 and pos are called leftStyleSet and rightStyleSet respectively.
    ///
    /// - If there is a style x that exists in leftStyleSet but not in rightStyleSet, it means that the position pos - 1 is the end anchor of x.
    ///   The newly inserted text is after the end anchor of x, so the StyleSet of the new text should not include this style.
    /// - If there is a style x that exists in rightStyleSet but not in leftStyleSet, it means that the position pos is the start anchor of x.
    ///   The newly inserted text is before the start anchor of x, so the StyleSet of the new text should not include this style.
    /// - If both leftStyleSet and rightStyleSet contain style x, it means that the newly inserted text is within the style range, so the StyleSet should include x.
    pub fn insert(&mut self, pos: usize, len: usize) {
        if pos == 0 {
            self.0.prepend(Elem {
                len,
                styles: Default::default(),
            });
            return;
        }

        if pos == *self.0.root_cache() {
            self.0.push(Elem {
                len,
                styles: Default::default(),
            });
            return;
        }

        let right = self.0.query::<LengthFinder>(&pos);
        let left = self.0.query::<LengthFinder>(&(pos - 1));
        if left.elem_index == right.elem_index && left.leaf == right.leaf {
            // left and right are in the same element, we can increase the length of the element directly
            self.0.get_elem_mut(&left).unwrap().len += len;
            return;
        }

        // insert by the intersection of left styles and right styles
        let mut styles = self.0.get_elem(&left).unwrap().styles.clone();
        let right_styles = &self.0.get_elem(&right).unwrap().styles;
        styles.retain(|key, value| {
            if let Some(right_value) = right_styles.get(key) {
                value.set.retain(|x| right_value.set.contains(x));
                return !value.set.is_empty();
            }

            false
        });

        self.0.insert_by_query_result(right, Elem { len, styles });
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
