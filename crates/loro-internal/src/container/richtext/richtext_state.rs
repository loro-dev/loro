use append_only_bytes::{AppendOnlyBytes, BytesSlice};
use fxhash::FxHashMap;
use generic_btree::{
    rle::{insert_with_split, HasLength, Mergeable, Sliceable},
    BTree, BTreeTrait, LengthFinder, Query, UseLengthFinder,
};
use loro_common::{Counter, LoroValue, PeerID, ID};
use smallvec::SmallVec;
use std::{
    borrow::Cow,
    ops::{Add, AddAssign, Range, RangeBounds, Sub},
    str::Utf8Error,
    sync::Arc,
};

use crate::{
    change::Lamport,
    container::{richtext::style_range_map::StyleValue, text::utf16::count_utf16_chars},
    InternalString, VersionVector,
};

use self::query::{EntityQueryT, UnicodeQuery};

use super::{
    query_by_len::{IndexQuery, QueryByLen},
    style_range_map::StyleRangeMap,
    tinyvec::TinyVec,
    RichtextSpan, Style, StyleInner, TextStyleInfo,
};

#[derive(Clone, Debug, Default)]
pub(crate) struct RichtextState {
    tree: BTree<RichtextTreeTrait>,
    style_ranges: StyleRangeMap,
}

#[derive(Clone, Debug)]
enum Elem {
    Text { unicode_len: i32, text: BytesSlice },
    Style(TinyVec<TextStyleInfo, 16>),
}

impl Elem {
    pub fn try_from_bytes(s: BytesSlice) -> Result<Self, Utf8Error> {
        Ok(Elem::Text {
            unicode_len: std::str::from_utf8(&s)?.chars().count() as i32,
            text: s,
        })
    }

    pub fn from_style(style: TextStyleInfo) -> Self {
        let mut v = TinyVec::new();
        v.push(style);
        Self::Style(v)
    }
}

impl HasLength for Elem {
    fn rle_len(&self) -> usize {
        match self {
            Elem::Text { unicode_len, text } => *unicode_len as usize,
            Elem::Style(data) => data.len(),
        }
    }
}

impl Mergeable for Elem {
    fn can_merge(&self, rhs: &Self) -> bool {
        match (self, rhs) {
            (Elem::Text { text: l, .. }, Elem::Text { text: r, .. }) => l.can_merge(r),
            (Elem::Style(l), Elem::Style(r)) => l.can_merge(r),
            _ => false,
        }
    }

    fn merge_right(&mut self, rhs: &Self) {
        match (self, rhs) {
            (
                Elem::Text { unicode_len, text },
                Elem::Text {
                    unicode_len: rhs_len,
                    text: rhs_text,
                },
            ) => {
                *unicode_len += *rhs_len;
                text.try_merge(rhs_text).unwrap();
            }
            (Elem::Style(a), Elem::Style(b)) => {
                a.merge(&b);
            }
            _ => unreachable!(),
        }
    }

    fn merge_left(&mut self, left: &Self) {
        match (self, left) {
            (
                Elem::Text { unicode_len, text },
                Elem::Text {
                    unicode_len: left_len,
                    text: left_text,
                },
            ) => {
                *unicode_len += *left_len;
                // TODO: small PERF improvement
                let mut new_text = left_text.clone();
                new_text.try_merge(text).unwrap();
                *text = new_text;
            }
            (Elem::Style(a), Elem::Style(b)) => {
                a.merge_left(&b);
            }
            _ => unreachable!(),
        }
    }
}

impl Sliceable for Elem {
    fn slice(&self, range: impl RangeBounds<usize>) -> Self {
        let start_index = match range.start_bound() {
            std::ops::Bound::Included(s) => *s,
            std::ops::Bound::Excluded(s) => *s + 1,
            std::ops::Bound::Unbounded => 0,
        };

        let end_index = match range.end_bound() {
            std::ops::Bound::Included(s) => *s + 1,
            std::ops::Bound::Excluded(s) => *s,
            std::ops::Bound::Unbounded => self.rle_len(),
        };

        let Elem::Text { unicode_len, text } = self else {
            return self.slice(start_index..end_index);
        };

        let s = std::str::from_utf8(text).unwrap();
        let from = unicode_to_byte_index(s, start_index).unwrap();
        let len = unicode_to_byte_index(&s[from..], end_index - start_index).unwrap();
        let to = from + len;
        return Elem::Text {
            unicode_len: (end_index - start_index) as i32,
            text: text.slice_clone(from..to),
        };
    }
}

fn unicode_to_byte_index(s: &str, unicode_index: usize) -> Option<usize> {
    let mut current_unicode_index = 0;
    for (byte_index, _) in s.char_indices() {
        if current_unicode_index == unicode_index {
            return Some(byte_index);
        }
        current_unicode_index += 1;
    }

    if current_unicode_index == unicode_index {
        return Some(s.len());
    }

    None
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Default)]
struct Cache {
    unicode_len: i32,
    utf16_len: i32,
    entity_len: i32,
}

impl AddAssign for Cache {
    fn add_assign(&mut self, rhs: Self) {
        self.unicode_len += rhs.unicode_len;
        self.utf16_len += rhs.utf16_len;
        self.entity_len += rhs.entity_len;
    }
}

impl Add for Cache {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            unicode_len: self.unicode_len + rhs.unicode_len,
            utf16_len: self.utf16_len + rhs.utf16_len,
            entity_len: self.entity_len + rhs.entity_len,
        }
    }
}

impl Sub for Cache {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            unicode_len: self.unicode_len - rhs.unicode_len,
            utf16_len: self.utf16_len - rhs.utf16_len,
            entity_len: self.entity_len - rhs.entity_len,
        }
    }
}

struct RichtextTreeTrait;

impl BTreeTrait for RichtextTreeTrait {
    type Elem = Elem;

    type Cache = Cache;

    type CacheDiff = Cache;

    const MAX_LEN: usize = 16;

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
                let mut new_cache = Cache::default();
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
        diff: Option<Self::CacheDiff>,
    ) -> Self::CacheDiff {
        match diff {
            Some(diff) => {
                *cache += diff;
                diff
            }
            None => {
                let mut new_cache = Cache::default();
                for elem in elements {
                    match elem {
                        Elem::Text { unicode_len, .. } => {
                            new_cache.unicode_len += unicode_len;
                            new_cache.utf16_len += unicode_len;
                            new_cache.entity_len += 1;
                        }
                        Elem::Style(size) => new_cache.entity_len += size.len() as i32,
                    }
                }

                let diff = new_cache - *cache;
                *cache = new_cache;
                diff
            }
        }
    }

    fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
        *diff1 += *diff2;
    }

    fn insert(
        elements: &mut generic_btree::HeapVec<Self::Elem>,
        index: usize,
        offset: usize,
        elem: Self::Elem,
    ) {
        insert_with_split(elements, index, offset, elem)
    }
}

// This query implementation will prefer right element when both left element and right element are valid.
mod query {
    use super::*;

    pub(super) struct UnicodeQueryT;
    pub(super) type UnicodeQuery = IndexQuery<UnicodeQueryT, RichtextTreeTrait>;

    impl QueryByLen<RichtextTreeTrait> for UnicodeQueryT {
        fn get_cache_len(cache: &<RichtextTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.unicode_len as usize
        }

        fn get_elem_len(elem: &<RichtextTreeTrait as BTreeTrait>::Elem) -> usize {
            match elem {
                Elem::Text { unicode_len, text } => *unicode_len as usize,
                Elem::Style(_) => 0,
            }
        }
    }

    pub(super) struct Utf16QueryT;
    pub(super) type Utf16Query = IndexQuery<Utf16QueryT, RichtextTreeTrait>;

    impl QueryByLen<RichtextTreeTrait> for Utf16QueryT {
        fn get_cache_len(cache: &<RichtextTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.utf16_len as usize
        }

        fn get_elem_len(elem: &<RichtextTreeTrait as BTreeTrait>::Elem) -> usize {
            match elem {
                Elem::Text {
                    unicode_len: _,
                    text,
                } => count_utf16_chars(text),
                Elem::Style(_) => 0,
            }
        }
    }

    pub(super) struct EntityQueryT;
    pub(super) type EntityQuery = IndexQuery<EntityQueryT, RichtextTreeTrait>;

    impl QueryByLen<RichtextTreeTrait> for EntityQueryT {
        fn get_cache_len(cache: &<RichtextTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.entity_len as usize
        }

        fn get_elem_len(elem: &<RichtextTreeTrait as BTreeTrait>::Elem) -> usize {
            match elem {
                Elem::Text {
                    unicode_len,
                    text: _,
                } => *unicode_len as usize,
                Elem::Style(data) => data.len(),
            }
        }
    }
}

impl RichtextState {
    /// Insert text at a unicode index. Return the entity index.
    pub(crate) fn insert(&mut self, pos: usize, text: BytesSlice) -> usize {
        let right = self.find_best_insert_pos_from_unicode_index(pos);
        let entity_index = self.get_entity_index_from_path(right);

        // TODO: find the best insert position
        let insert_pos = right;
        let elem = Elem::try_from_bytes(text).unwrap();
        self.style_ranges.insert(entity_index, elem.rle_len());
        self.tree.insert_by_query_result(insert_pos, elem);

        entity_index
    }

    fn find_best_insert_pos_from_unicode_index(
        &mut self,
        pos: usize,
    ) -> generic_btree::QueryResult {
        self.tree.query::<UnicodeQuery>(&pos)
        // TODO: impl peritext find pos
    }

    fn get_entity_index_from_path(&mut self, right: generic_btree::QueryResult) -> usize {
        let mut entity_index = 0;
        self.tree.visit_previous_caches(right, |cache| match cache {
            generic_btree::PreviousCache::NodeCache(cache) => {
                entity_index += EntityQueryT::get_cache_len(cache);
            }
            generic_btree::PreviousCache::PrevSiblingElem(elem) => {
                entity_index += EntityQueryT::get_elem_len(elem);
            }
            generic_btree::PreviousCache::ThisElemAndOffset { elem: _, offset } => {
                entity_index += offset;
            }
        });
        entity_index
    }

    /// Delete a range of text.
    ///
    /// Delete a range of text. (The style anchors included in the range are not deleted.)
    pub(crate) fn delete(&mut self, pos: usize, len: usize) -> Vec<Range<usize>> {
        let mut style_anchors: Vec<Elem> = Vec::new();
        let mut removed_entity_ranges: Vec<Range<usize>> = Vec::new();
        let q = self.tree.query::<UnicodeQuery>(&pos);
        let mut entity_index = self.get_entity_index_from_path(q);
        let mut deleted = 0;

        for span in self.tree.drain::<UnicodeQuery>(pos..pos + len) {
            match span {
                Elem::Style(style) => {
                    entity_index += style.len();
                    if let Some(last) = style_anchors.last_mut() {
                        let Elem::Style(last) = last else {
                            unreachable!()
                        };
                        if last.can_merge(&style) {
                            last.merge(&style);
                        } else {
                            style_anchors.push(Elem::Style(style));
                        }
                    } else {
                        style_anchors.push(Elem::Style(style));
                    }
                }
                Elem::Text {
                    unicode_len,
                    text: _,
                } => {
                    self.style_ranges.delete(
                        entity_index - deleted..entity_index - deleted + unicode_len as usize,
                    );
                    deleted += unicode_len as usize;
                    if let Some(last) = removed_entity_ranges.last_mut() {
                        if last.end == entity_index {
                            last.end += unicode_len as usize;
                        } else {
                            removed_entity_ranges
                                .push(entity_index..(entity_index + unicode_len as usize));
                        }
                    } else {
                        removed_entity_ranges
                            .push(entity_index..(entity_index + unicode_len as usize));
                    }

                    entity_index += unicode_len as usize;
                }
            }
        }

        let q = self.tree.query::<UnicodeQuery>(&pos);
        self.tree.insert_many_by_query_result(&q, style_anchors);

        removed_entity_ranges
    }

    /// Mark a range of text with a style.
    ///
    /// Return the corresponding entity index ranges.
    pub(crate) fn mark(&mut self, range: Range<usize>, style: Arc<StyleInner>) -> Range<usize> {
        let end_pos = self.find_best_insert_pos_from_unicode_index(range.end);
        let end_entity_index = self.get_entity_index_from_path(end_pos);
        self.tree
            .insert_by_query_result(end_pos, Elem::from_style(style.info.to_end()));

        let start_pos = self.find_best_insert_pos_from_unicode_index(range.start);
        let start_entity_index = self.get_entity_index_from_path(start_pos);
        self.tree
            .insert_by_query_result(start_pos, Elem::from_style(style.info.to_start()));

        self.style_ranges.insert(end_entity_index, 1);
        self.style_ranges.insert(start_entity_index, 1);
        self.style_ranges
            .annotate(start_entity_index..end_entity_index + 1, style);

        start_entity_index..end_entity_index
    }

    pub fn iter(&self) -> impl Iterator<Item = RichtextSpan<'_>> {
        let mut entity_index = 0;
        let mut style_range_iter = self.style_ranges.iter();
        let mut cur_range = style_range_iter.next();

        fn to_styles(
            (_, style_map): &(Range<usize>, &FxHashMap<InternalString, StyleValue>),
        ) -> Vec<Style> {
            let mut styles = Vec::with_capacity(style_map.len());
            for style in style_map.iter().flat_map(|(_, values)| values.to_styles()) {
                styles.push(style);
            }
            styles
        }

        let mut cur_styles = cur_range.as_ref().map(to_styles);

        self.tree.iter().filter_map(move |x| match x {
            Elem::Text { unicode_len, text } => {
                let mut styles = Vec::new();
                while let Some((inner_cur_range, _)) = cur_range.as_ref() {
                    if entity_index < inner_cur_range.start {
                        break;
                    }

                    if entity_index < inner_cur_range.end {
                        styles = cur_styles.as_ref().unwrap().clone();
                        break;
                    } else {
                        cur_range = style_range_iter.next();
                        cur_styles = cur_range.as_ref().map(to_styles);
                    }
                }

                entity_index += *unicode_len as usize;
                Some(RichtextSpan {
                    // SAFETY: We know for sure text is valid utf8
                    text: Cow::Borrowed(unsafe { std::str::from_utf8_unchecked(text.as_bytes()) }),
                    styles,
                })
            }
            Elem::Style(s) => {
                entity_index += s.len();
                None
            }
        })
    }

    pub fn to_vec(&self) -> Vec<RichtextSpan<'_>> {
        self.iter().collect()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Debug, Default, Clone)]
    struct SimpleWrapper {
        state: RichtextState,
        bytes: AppendOnlyBytes,
    }

    impl SimpleWrapper {
        fn insert(&mut self, pos: usize, text: &str) {
            let start = self.bytes.len();
            self.bytes.push_str(text);
            self.state.insert(pos, self.bytes.slice(start..));
        }
    }

    #[test]
    fn test() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello World!");
        wrapper.state.mark(
            0..5,
            Arc::new(StyleInner::new_for_test(0, TextStyleInfo::BOLD)),
        );
        assert_eq!(
            wrapper.state.to_vec(),
            vec![
                RichtextSpan {
                    text: Cow::Borrowed("Hello"),
                    styles: vec![Style {
                        key: "0".into(),
                        data: LoroValue::Null
                    }]
                },
                RichtextSpan {
                    text: Cow::Borrowed(" World!"),
                    styles: vec![]
                }
            ]
        );
        wrapper.state.mark(
            2..7,
            Arc::new(StyleInner::new_for_test(1, TextStyleInfo::LINK)),
        );
        assert_eq!(
            wrapper.state.to_vec(),
            vec![
                RichtextSpan {
                    text: Cow::Borrowed("He"),
                    styles: vec![Style {
                        key: "0".into(),
                        data: LoroValue::Null
                    }]
                },
                RichtextSpan {
                    text: Cow::Borrowed("llo"),
                    styles: vec![
                        Style {
                            key: "0".into(),
                            data: LoroValue::Null,
                        },
                        Style {
                            key: "1".into(),
                            data: LoroValue::Null,
                        }
                    ]
                },
                RichtextSpan {
                    text: Cow::Borrowed(" W"),
                    styles: vec![Style {
                        key: "1".into(),
                        data: LoroValue::Null,
                    }]
                },
                RichtextSpan {
                    text: Cow::Borrowed("orld!"),
                    styles: vec![]
                }
            ]
        );
    }
}
