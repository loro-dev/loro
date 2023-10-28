//! This map a Range<usize> to a set of style
//!

use std::{
    collections::{BTreeSet, HashMap},
    ops::Range,
    sync::Arc,
    usize,
};

use fxhash::FxHashMap;
use generic_btree::{
    rle::{HasLength, Mergeable, Sliceable},
    BTree, BTreeTrait, LengthFinder, UseLengthFinder,
};
use once_cell::sync::Lazy;

use crate::InternalString;

use super::{Style, StyleOp};

/// This struct keep the mapping of ranges to numbers
///
/// It's initialized with usize::MAX/2 length.
#[derive(Debug, Clone)]
pub(super) struct StyleRangeMap {
    pub(super) tree: BTree<RangeNumMapTrait>,
    has_style: bool,
}

#[derive(Debug, Clone)]
pub(super) struct RangeNumMapTrait;

pub(crate) type Styles = FxHashMap<InternalString, StyleValue>;

pub(super) static EMPTY_STYLES: Lazy<Styles> =
    Lazy::new(|| HashMap::with_hasher(Default::default()));

#[derive(Debug, Clone)]
pub(super) struct Elem {
    styles: Styles,
    len: usize,
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct StyleValue {
    set: BTreeSet<Arc<StyleOp>>,
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

    // PERF: can we avoid this box
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
        tree.push(Elem {
            styles: Default::default(),
            len: usize::MAX / 4,
        });

        Self {
            tree,
            has_style: false,
        }
    }

    pub fn annotate(&mut self, range: Range<usize>, style: Arc<StyleOp>) {
        debug_log::debug_log!("Annotate {:?}", &range);
        let range = self.tree.range::<LengthFinder>(range);
        if range.is_none() {
            unreachable!();
        }

        self.has_style = true;
        let range = range.unwrap();
        debug_log::debug_log!("Range={:?}", &range);
        self.tree
            .update(range.start.cursor..range.end.cursor, &mut |x| {
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

                None
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
    pub fn insert(&mut self, pos: usize, len: usize) -> &Styles {
        if !self.has_style {
            return &EMPTY_STYLES;
        }

        if pos == 0 {
            self.tree.prepend(Elem {
                len,
                styles: Default::default(),
            });
            return &EMPTY_STYLES;
        }

        if pos as isize == *self.tree.root_cache() {
            self.tree.push(Elem {
                len,
                styles: Default::default(),
            });
            return &EMPTY_STYLES;
        }

        let right = self.tree.query::<LengthFinder>(&pos).unwrap().cursor;
        let left = self.tree.query::<LengthFinder>(&(pos - 1)).unwrap().cursor;
        if left.leaf == right.leaf {
            // left and right are in the same element, we can increase the length of the element directly
            self.tree.update_leaf(left.leaf, |x| {
                x.len += len;
                (true, None, None)
            });
            return &self.tree.get_elem(left.leaf).unwrap().styles;
        }

        // insert by the intersection of left styles and right styles
        let mut styles = self.tree.get_elem(left.leaf).unwrap().styles.clone();
        let right_styles = &self.tree.get_elem(right.leaf).unwrap().styles;
        styles.retain(|key, value| {
            if let Some(right_value) = right_styles.get(key) {
                value.set.retain(|x| right_value.set.contains(x));
                return !value.set.is_empty();
            }

            false
        });

        self.tree.insert_by_path(right, Elem { len, styles });
        return &self.tree.get_elem(right.leaf).unwrap().styles;
    }

    pub fn get(&mut self, index: usize) -> Option<&FxHashMap<InternalString, StyleValue>> {
        if !self.has_style {
            return None;
        }

        let result = self.tree.query::<LengthFinder>(&index)?.cursor;
        self.tree.get_elem(result.leaf).map(|x| &x.styles)
    }

    pub fn iter(
        &self,
    ) -> impl Iterator<Item = (Range<usize>, &FxHashMap<InternalString, StyleValue>)> + '_ {
        let mut index = 0;
        self.tree.iter().filter_map(move |elem| {
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

    pub fn iter_from(
        &self,
        start_entity_index: usize,
    ) -> impl Iterator<Item = (Range<usize>, &FxHashMap<InternalString, StyleValue>)> + '_ {
        let start = self
            .tree
            .query::<LengthFinder>(&start_entity_index)
            .unwrap();
        let mut index = start_entity_index - start.offset();
        self.tree
            .iter_range(start.cursor()..)
            .filter_map(move |elem| {
                let len = elem.elem.len;
                let value = &elem.elem.styles;
                let range = index.max(start_entity_index)..index + len;
                index += len;
                if elem.elem.styles.is_empty() {
                    return None;
                }

                Some((range, value))
            })
    }

    pub fn delete(&mut self, range: Range<usize>) {
        if !self.has_style {
            return;
        }

        let start = self.tree.query::<LengthFinder>(&range.start).unwrap();
        let end = self.tree.query::<LengthFinder>(&range.end).unwrap();
        if start.cursor.leaf == end.cursor.leaf {
            // delete in the same element
            self.tree.update_leaf(start.cursor.leaf, |mut x| {
                x.len -= range.len();
                (true, None, None)
            });
            return;
        }

        self.tree.drain(start..end);
    }

    pub fn len(&self) -> usize {
        *self.tree.root_cache() as usize
    }

    pub(crate) fn has_style(&self) -> bool {
        self.has_style
    }
}

impl UseLengthFinder<RangeNumMapTrait> for RangeNumMapTrait {
    fn get_len(cache: &isize) -> usize {
        *cache as usize
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
    fn _slice(&self, range: std::ops::Range<usize>) -> Self {
        let len = range.len();
        Elem {
            styles: self.styles.clone(),
            len,
        }
    }
}

impl BTreeTrait for RangeNumMapTrait {
    type Elem = Elem;
    type Cache = isize;
    type CacheDiff = isize;

    fn calc_cache_internal(
        cache: &mut Self::Cache,
        caches: &[generic_btree::Child<Self>],
    ) -> isize {
        let new_cache = caches.iter().map(|c| c.cache).sum();
        let diff = new_cache - *cache;
        *cache = new_cache;
        diff
    }

    fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
        *diff1 += diff2;
    }

    fn apply_cache_diff(cache: &mut Self::Cache, diff: &Self::CacheDiff) {
        *cache += diff;
    }

    fn get_elem_cache(elem: &Self::Elem) -> Self::Cache {
        elem.len as isize
    }

    fn new_cache_to_diff(cache: &Self::Cache) -> Self::CacheDiff {
        *cache
    }

    fn sub_cache(cache_lhs: &Self::Cache, cache_rhs: &Self::Cache) -> Self::CacheDiff {
        *cache_lhs - *cache_rhs
    }
}

#[cfg(test)]
mod test {
    use loro_common::PeerID;

    use crate::{change::Lamport, container::richtext::TextStyleInfoFlag};

    use super::*;

    fn new_style(n: i32) -> Arc<StyleOp> {
        Arc::new(StyleOp {
            lamport: n as Lamport,
            peer: n as PeerID,
            cnt: n,
            key: n.to_string().into(),
            info: TextStyleInfoFlag::default(),
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