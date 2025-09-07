//! This map a Range<usize> to a set of style
//!

use std::{
    collections::BTreeSet,
    ops::{ControlFlow, Deref, DerefMut, Range, RangeBounds},
    sync::Arc,
};

use rustc_hash::FxHashMap;
use generic_btree::{
    rle::{CanRemove, HasLength, Mergeable, Sliceable, TryInsert},
    BTree, BTreeTrait, ElemSlice, LengthFinder, UseLengthFinder,
};

use once_cell::sync::Lazy;

use crate::delta::StyleMeta;

use super::{AnchorType, StyleKey, StyleOp};

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

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Styles {
    pub(crate) styles: FxHashMap<StyleKey, StyleValue>,
}

impl Styles {
    pub(crate) fn has_key_value(&self, key: &str, value: &loro_common::LoroValue) -> bool {
        match self.get(&StyleKey::Key(key.into())) {
            Some(v) => match v.get() {
                Some(v) => &v.value == value,
                _ => false,
            },
            _ => false,
        }
    }

    /// Infer the anchors between the neighbor styles.
    /// Returns the last anchor of the left style and the first anchor of the right style.
    fn infer_anchors(&self, next: &Self) -> (Option<Arc<StyleOp>>, Option<Arc<StyleOp>>) {
        let mut left_anchor = None;
        let mut right_anchor = None;
        let empty_set: BTreeSet<_> = Default::default();
        for (key, set) in self.styles.iter() {
            let right_set = next.styles.get(key).map(|x| &x.set).unwrap_or(&empty_set);
            for diff in set.set.difference(right_set) {
                assert!(left_anchor.is_none(), "left anchor should be unique");
                left_anchor = Some(diff.clone());
            }
        }

        for (key, set) in next.styles.iter() {
            let left_set = self.styles.get(key).map(|x| &x.set).unwrap_or(&empty_set);
            for diff in set.set.difference(left_set) {
                assert!(right_anchor.is_none(), "right anchor should be unique");
                right_anchor = Some(diff.clone());
            }
        }

        (left_anchor, right_anchor)
    }
}

impl Deref for Styles {
    type Target = FxHashMap<StyleKey, StyleValue>;

    fn deref(&self) -> &Self::Target {
        &self.styles
    }
}

impl DerefMut for Styles {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.styles
    }
}

pub(super) static EMPTY_STYLES: Lazy<Styles> = Lazy::new(Default::default);

#[derive(Debug, Clone)]
pub(crate) struct Elem {
    pub(crate) styles: Styles,
    pub(crate) len: usize,
}

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub(crate) struct StyleValue {
    // we need a set here because we need to calculate the intersection of styles when
    // users insert new text between two style sets
    set: BTreeSet<Arc<StyleOp>>,
}

impl StyleValue {
    pub fn insert(&mut self, value: Arc<StyleOp>) {
        self.set.insert(value);
    }

    pub fn get(&self) -> Option<&Arc<StyleOp>> {
        self.set.last()
    }
}

impl Default for StyleRangeMap {
    fn default() -> Self {
        Self::new()
    }
}

type YieldStyle<'a> = Option<&'a mut dyn FnMut(&Styles, usize)>;

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

    pub fn annotate(
        &mut self,
        range: Range<usize>,
        style: Arc<StyleOp>,
        mut yield_style: YieldStyle,
    ) {
        let range = self.tree.range::<LengthFinder>(range);
        if range.is_none() {
            unreachable!();
        }

        self.has_style = true;
        let range = range.unwrap();
        self.tree
            .update(range.start.cursor..range.end.cursor, &mut |x| {
                if let Some(set) = x.styles.get_mut(&style.get_style_key()) {
                    set.set.insert(style.clone());
                } else {
                    let key = style.get_style_key();
                    let mut value = StyleValue::default();
                    value.insert(style.clone());
                    x.styles.insert(key, value);
                }

                if let Some(y) = yield_style.as_mut() {
                    y(&x.styles, x.len);
                }
                None
            });
    }

    /// Get the styles of the range. If the range is not in the same leaf, return None.
    pub(crate) fn get_styles_of_range(&self, range: Range<usize>) -> Option<&Styles> {
        if !self.has_style {
            return None;
        }

        let right = self
            .tree
            .query::<LengthFinder>(&(range.end - 1))
            .unwrap()
            .cursor;
        let left = self
            .tree
            .query::<LengthFinder>(&range.start)
            .unwrap()
            .cursor;
        if left.leaf == right.leaf {
            Some(&self.tree.get_elem(left.leaf).unwrap().styles)
        } else {
            None
        }
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

        let (target, _) = self.tree.insert_by_path(right, Elem { len, styles });
        &self.tree.get_elem(target.leaf).unwrap().styles
    }

    /// Return the style sets beside `index` and get the intersection of them.
    pub fn get_styles_for_insert(&self, index: usize) -> StyleMeta {
        if index == 0 || !self.has_style {
            return StyleMeta::default();
        }

        let left = self
            .tree
            .query::<LengthFinder>(&(index - 1))
            .unwrap()
            .cursor;
        let right = self.tree.shift_path_by_one_offset(left).unwrap();
        if left.leaf == right.leaf {
            let styles = &self.tree.get_elem(left.leaf).unwrap().styles;
            styles.into()
        } else {
            let mut styles = self.tree.get_elem(left.leaf).unwrap().styles.clone();
            let right_styles = &self.tree.get_elem(right.leaf).unwrap().styles;
            styles.retain(|key, value| {
                if let Some(right_value) = right_styles.get(key) {
                    value.set.retain(|x| right_value.set.contains(x));
                    return !value.set.is_empty();
                }

                false
            });

            styles.into()
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (Range<usize>, &Styles)> + '_ {
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

    /// Update the styles from `pos` to the start of the document.
    fn update_styles_scanning_backward(
        &mut self,
        pos: usize,
        mut f: impl FnMut(&mut Elem) -> ControlFlow<()>,
    ) {
        let mut cursor = self.tree.query::<LengthFinder>(&pos).map(|x| x.cursor);
        while let Some(inner_cursor) = cursor {
            cursor = self.tree.prev_elem(inner_cursor);
            let node = self.tree.get_elem_mut(inner_cursor.leaf).unwrap();
            match f(node) {
                ControlFlow::Continue(_) => {}
                ControlFlow::Break(_) => {
                    break;
                }
            }
        }
    }

    pub(crate) fn iter_range(
        &self,
        range: impl RangeBounds<usize>,
    ) -> impl Iterator<Item = ElemSlice<'_, Elem>> + '_ {
        let start = match range.start_bound() {
            std::ops::Bound::Included(x) => *x,
            std::ops::Bound::Excluded(x) => *x + 1,
            std::ops::Bound::Unbounded => 0,
        };

        let end = match range.end_bound() {
            std::ops::Bound::Included(x) => *x + 1,
            std::ops::Bound::Excluded(x) => *x,
            std::ops::Bound::Unbounded => usize::MAX,
        };

        let start = self.tree.query::<LengthFinder>(&start).unwrap();
        let end = self.tree.query::<LengthFinder>(&end).unwrap();
        self.tree.iter_range(start.cursor..end.cursor)
    }

    /// Return the expected style anchors with their indexes.
    pub(super) fn iter_anchors(&self) -> impl Iterator<Item = IterAnchorItem> + '_ {
        let mut index = 0;
        let empty_styles = &EMPTY_STYLES;
        let mut last: Option<&Elem> = None;
        let mut vec = Vec::new();
        for cur in self.tree.iter() {
            let last_styles = last.map(|x| &x.styles).unwrap_or(empty_styles);
            let (left_anchor, right_anchor) = last_styles.infer_anchors(&cur.styles);
            if let Some(left) = left_anchor {
                vec.push(IterAnchorItem {
                    index: index - 1,
                    op: left.clone(),
                    anchor_type: AnchorType::End,
                });
            }
            if let Some(right) = right_anchor {
                vec.push(IterAnchorItem {
                    index,
                    op: right.clone(),
                    anchor_type: AnchorType::Start,
                });
            }

            last = Some(cur);
            index += cur.len;
        }

        let last_styles = last.map(|x| &x.styles).unwrap_or(empty_styles);
        let (left_anchor, right_anchor) = last_styles.infer_anchors(empty_styles);
        if let Some(left) = left_anchor {
            vec.push(IterAnchorItem {
                index: index - 1,
                op: left.clone(),
                anchor_type: AnchorType::End,
            });
        }
        if let Some(right) = right_anchor {
            vec.push(IterAnchorItem {
                index,
                op: right.clone(),
                anchor_type: AnchorType::Start,
            });
        }

        vec.into_iter()
    }

    /// Remove the style scanning backward, return the start_entity_index
    pub fn remove_style_scanning_backward(
        &mut self,
        to_remove: &Arc<StyleOp>,
        last_index: usize,
    ) -> usize {
        let mut removed_len = 0;
        self.update_styles_scanning_backward(last_index, |elem| {
            removed_len += elem.len;
            let styles = &mut elem.styles;
            let key = to_remove.get_style_key();
            let mut has_removed = false;
            if let Some(value) = styles.get_mut(&key) {
                has_removed = value.set.remove(to_remove);
                if value.set.is_empty() {
                    styles.remove(&key);
                }
            }

            if has_removed {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            }
        });

        last_index + 1 - removed_len
    }

    pub fn delete(&mut self, range: Range<usize>) {
        if !self.has_style {
            return;
        }

        let start = self.tree.query::<LengthFinder>(&range.start).unwrap();
        let end = self.tree.query::<LengthFinder>(&range.end).unwrap();
        if start.cursor.leaf == end.cursor.leaf {
            // delete in the same element
            self.tree.update_leaf(start.cursor.leaf, |x| {
                x.len -= range.len();
                (true, None, None)
            });
            return;
        }

        self.tree.drain(start..end);
    }

    pub(crate) fn has_style(&self) -> bool {
        self.has_style
    }

    pub(crate) fn estimate_size(&self) -> usize {
        // TODO: this is inaccurate
        self.tree.node_len() * std::mem::size_of::<Elem>()
    }
}

pub(super) struct IterAnchorItem {
    pub(super) index: usize,
    pub(super) op: Arc<StyleOp>,
    pub(super) anchor_type: AnchorType,
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

impl TryInsert for Elem {
    fn try_insert(&mut self, _pos: usize, elem: Self) -> Result<(), Self>
    where
        Self: Sized,
    {
        if self.styles == elem.styles {
            self.len += elem.len;
            Ok(())
        } else {
            Err(elem)
        }
    }
}

impl CanRemove for Elem {
    fn can_remove(&self) -> bool {
        self.len == 0
    }
}

impl BTreeTrait for RangeNumMapTrait {
    type Elem = Elem;
    type Cache = isize;
    type CacheDiff = isize;
    const USE_DIFF: bool = true;

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
            value: loro_common::LoroValue::Bool(true),
        })
    }

    #[test]
    fn test_basic_insert() {
        let mut map = StyleRangeMap::default();
        map.annotate(1..10, new_style(1), None);
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
        map.annotate(1..10, new_style(1), None);
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
