use append_only_bytes::BytesSlice;
use fxhash::{FxHashMap, FxHashSet};
use generic_btree::{
    rle::{HasLength, Mergeable, Sliceable},
    BTree, BTreeTrait, Cursor, LeafDirtyMap, Query,
};
use loro_common::LoroValue;
use serde::{ser::SerializeStruct, Serialize};
use std::fmt::{Display, Formatter};
use std::{
    ops::{Add, AddAssign, Range, Sub},
    str::{from_utf8_unchecked, Utf8Error},
    sync::Arc,
};

use crate::{
    container::richtext::style_range_map::StyleValue, delta::DeltaValue,
    utils::utf16::count_utf16_chars, InternalString,
};

// FIXME: Check splice and other things are using unicode index
use self::query::{EntityQuery, EntityQueryT, EventIndexQuery, UnicodeQuery};

use super::{
    query_by_len::{IndexQuery, QueryByLen},
    style_range_map::{StyleRangeMap, Styles, EMPTY_STYLES},
    AnchorType, RichtextSpan, Style, StyleOp,
};

#[derive(Clone, Debug, Default)]
pub(crate) struct RichtextState {
    tree: BTree<RichtextTreeTrait>,
    style_ranges: StyleRangeMap,
}

impl Display for RichtextState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for span in self.tree.iter() {
            match span {
                RichtextStateChunk::Style { .. } => {}
                RichtextStateChunk::Text { text, .. } => {
                    f.write_str(std::str::from_utf8(text).unwrap())?;
                }
            }
        }

        Ok(())
    }
}

// TODO: change visibility back to crate after #116 is done
#[derive(Clone, Debug)]
pub enum RichtextStateChunk {
    Text {
        unicode_len: i32,
        text: BytesSlice,
    },
    Style {
        style: Arc<StyleOp>,
        anchor_type: AnchorType,
    },
}

impl RichtextStateChunk {
    pub fn new_text(s: BytesSlice) -> Self {
        Self::Text {
            unicode_len: std::str::from_utf8(&s).unwrap().chars().count() as i32,
            text: s,
        }
    }

    pub fn new_style(style: Arc<StyleOp>, anchor_type: AnchorType) -> Self {
        Self::Style { style, anchor_type }
    }
}

impl DeltaValue for RichtextStateChunk {
    fn value_extend(&mut self, other: Self) -> Result<(), Self> {
        Err(other)
    }

    fn take(&mut self, length: usize) -> Self {
        let mut right = self.split(length);
        std::mem::swap(self, &mut right);
        right
    }

    fn length(&self) -> usize {
        self.rle_len()
    }
}

impl Serialize for RichtextStateChunk {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            RichtextStateChunk::Text { unicode_len, .. } => {
                let mut state = serializer.serialize_struct("RichtextStateChunk", 3)?;
                state.serialize_field("type", "Text")?;
                state.serialize_field("unicode_len", unicode_len)?;
                state.serialize_field("text", self.as_str().unwrap())?;
                state.end()
            }
            RichtextStateChunk::Style { style, anchor_type } => {
                let mut state = serializer.serialize_struct("RichtextStateChunk", 3)?;
                state.serialize_field("type", "Style")?;
                state.serialize_field("style", &style.key)?;
                state.serialize_field("anchor_type", anchor_type)?;
                state.end()
            }
        }
    }
}

impl RichtextStateChunk {
    pub fn try_from_bytes(s: BytesSlice) -> Result<Self, Utf8Error> {
        Ok(RichtextStateChunk::Text {
            unicode_len: std::str::from_utf8(&s)?.chars().count() as i32,
            text: s,
        })
    }

    pub fn from_style(style: Arc<StyleOp>, anchor_type: AnchorType) -> Self {
        Self::Style { style, anchor_type }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            RichtextStateChunk::Text { text, .. } => {
                // SAFETY: We know that the text is valid UTF-8
                Some(unsafe { std::str::from_utf8_unchecked(text) })
            }
            _ => None,
        }
    }
}

impl HasLength for RichtextStateChunk {
    fn rle_len(&self) -> usize {
        match self {
            RichtextStateChunk::Text { unicode_len, text } => *unicode_len as usize,
            RichtextStateChunk::Style { .. } => 1,
        }
    }
}

impl Mergeable for RichtextStateChunk {
    fn can_merge(&self, rhs: &Self) -> bool {
        match (self, rhs) {
            (
                RichtextStateChunk::Text { text: l, .. },
                RichtextStateChunk::Text { text: r, .. },
            ) => l.can_merge(r),
            _ => false,
        }
    }

    fn merge_right(&mut self, rhs: &Self) {
        match (self, rhs) {
            (
                RichtextStateChunk::Text { unicode_len, text },
                RichtextStateChunk::Text {
                    unicode_len: rhs_len,
                    text: rhs_text,
                },
            ) => {
                *unicode_len += *rhs_len;
                text.try_merge(rhs_text).unwrap();
            }
            _ => unreachable!(),
        }
    }

    fn merge_left(&mut self, left: &Self) {
        match (self, left) {
            (
                RichtextStateChunk::Text { unicode_len, text },
                RichtextStateChunk::Text {
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
            _ => unreachable!(),
        }
    }
}

impl Sliceable for RichtextStateChunk {
    fn _slice(&self, range: Range<usize>) -> Self {
        let start_index = range.start;
        let end_index = range.end;

        let text = match self {
            RichtextStateChunk::Text {
                unicode_len: _,
                text,
            } => text,
            RichtextStateChunk::Style { style, anchor_type } => {
                assert_eq!(start_index, 0);
                assert_eq!(end_index, 1);
                return RichtextStateChunk::Style {
                    style: style.clone(),
                    anchor_type: *anchor_type,
                };
            }
        };

        let s = std::str::from_utf8(text).unwrap();
        let from = unicode_to_utf8_index(s, start_index).unwrap();
        let len = unicode_to_utf8_index(&s[from..], end_index - start_index).unwrap();
        let to = from + len;
        RichtextStateChunk::Text {
            unicode_len: (end_index - start_index) as i32,
            text: text.slice_clone(from..to),
        }
    }

    fn split(&mut self, pos: usize) -> Self {
        match self {
            RichtextStateChunk::Text { unicode_len, text } => {
                let s = std::str::from_utf8(text).unwrap();
                let byte_pos = unicode_to_utf8_index(s, pos).unwrap();
                let right = text.slice_clone(byte_pos..);
                let ans = RichtextStateChunk::Text {
                    unicode_len: *unicode_len - pos as i32,
                    text: right,
                };
                *text = text.slice_clone(..byte_pos);
                *unicode_len = pos as i32;
                ans
            }
            RichtextStateChunk::Style { .. } => {
                unreachable!()
            }
        }
    }
}

pub(crate) fn unicode_to_utf8_index(s: &str, unicode_index: usize) -> Option<usize> {
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

pub(crate) fn utf16_to_utf8_index(s: &str, utf16_index: usize) -> Option<usize> {
    let mut current_utf16_index = 0;
    for (byte_index, c) in s.char_indices() {
        let len = c.len_utf16();
        current_utf16_index += len;
        if current_utf16_index == utf16_index {
            return Some(byte_index + c.len_utf8());
        }
    }

    if current_utf16_index == utf16_index {
        return Some(s.len());
    }

    None
}

pub(crate) fn utf16_to_unicode_index(s: &str, utf16_index: usize) -> Option<usize> {
    if utf16_index == 0 {
        return Some(0);
    }

    let mut current_utf16_index = 0;
    for (i, c) in s.chars().enumerate() {
        let len = c.len_utf16();
        current_utf16_index += len;
        if current_utf16_index == utf16_index {
            return Some(i + 1);
        }
    }

    None
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Default)]
pub(crate) struct Cache {
    unicode_len: i32,
    bytes: i32,
    utf16_len: i32,
    entity_len: i32,
}

impl AddAssign for Cache {
    fn add_assign(&mut self, rhs: Self) {
        self.unicode_len += rhs.unicode_len;
        self.bytes += rhs.bytes;
        self.utf16_len += rhs.utf16_len;
        self.entity_len += rhs.entity_len;
    }
}

impl Add for Cache {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            bytes: self.bytes + rhs.bytes,
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
            bytes: self.bytes - rhs.bytes,
            unicode_len: self.unicode_len - rhs.unicode_len,
            utf16_len: self.utf16_len - rhs.utf16_len,
            entity_len: self.entity_len - rhs.entity_len,
        }
    }
}

pub(crate) struct RichtextTreeTrait;

impl BTreeTrait for RichtextTreeTrait {
    type Elem = RichtextStateChunk;

    type Cache = Cache;

    type CacheDiff = Cache;

    fn calc_cache_internal(
        cache: &mut Self::Cache,
        caches: &[generic_btree::Child<Self>],
    ) -> Self::CacheDiff {
        let mut new_cache = Cache::default();
        for child in caches {
            new_cache += child.cache;
        }

        let diff = new_cache - *cache;
        *cache = new_cache;
        diff
    }

    #[inline(always)]
    fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
        *diff1 += *diff2;
    }

    #[inline(always)]
    fn apply_cache_diff(cache: &mut Self::Cache, diff: &Self::CacheDiff) {
        *cache += *diff;
    }

    #[inline]
    fn get_elem_cache(elem: &Self::Elem) -> Self::Cache {
        match elem {
            RichtextStateChunk::Text { unicode_len, text } => Cache {
                bytes: text.len() as i32,
                unicode_len: *unicode_len,
                utf16_len: count_utf16_chars(text) as i32,
                entity_len: *unicode_len,
            },
            RichtextStateChunk::Style { .. } => Cache {
                bytes: 0,
                unicode_len: 0,
                utf16_len: 0,
                entity_len: 1,
            },
        }
    }

    #[inline(always)]
    fn new_cache_to_diff(cache: &Self::Cache) -> Self::CacheDiff {
        *cache
    }

    #[inline(always)]
    fn sub_cache(cache_lhs: &Self::Cache, cache_rhs: &Self::Cache) -> Self::CacheDiff {
        Cache {
            bytes: cache_lhs.bytes - cache_rhs.bytes,
            unicode_len: cache_lhs.unicode_len - cache_rhs.unicode_len,
            utf16_len: cache_lhs.utf16_len - cache_rhs.utf16_len,
            entity_len: cache_lhs.entity_len - cache_rhs.entity_len,
        }
    }
}

// This query implementation will prefer right element when both left element and right element are valid.
pub(crate) mod query {
    use super::*;

    #[cfg(not(feature = "wasm"))]
    pub(crate) type EventIndexQuery = UnicodeQuery;
    #[cfg(feature = "wasm")]
    pub(crate) type EventIndexQuery = Utf16Query;

    pub(crate) struct UnicodeQueryT;
    pub(crate) type UnicodeQuery = IndexQuery<UnicodeQueryT, RichtextTreeTrait>;

    impl QueryByLen<RichtextTreeTrait> for UnicodeQueryT {
        fn get_cache_len(cache: &<RichtextTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.unicode_len as usize
        }

        fn get_elem_len(elem: &<RichtextTreeTrait as BTreeTrait>::Elem) -> usize {
            match elem {
                RichtextStateChunk::Text {
                    unicode_len,
                    text: _,
                } => *unicode_len as usize,
                RichtextStateChunk::Style { .. } => 0,
            }
        }

        fn get_offset_and_found(
            left: usize,
            elem: &<RichtextTreeTrait as BTreeTrait>::Elem,
        ) -> (usize, bool) {
            match elem {
                RichtextStateChunk::Text {
                    unicode_len,
                    text: _,
                } => {
                    if *unicode_len as usize >= left {
                        return (left, true);
                    }

                    (left, false)
                }
                RichtextStateChunk::Style { .. } => (1, false),
            }
        }
    }

    pub(crate) struct Utf16QueryT;
    pub(crate) type Utf16Query = IndexQuery<Utf16QueryT, RichtextTreeTrait>;

    impl QueryByLen<RichtextTreeTrait> for Utf16QueryT {
        fn get_cache_len(cache: &<RichtextTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.utf16_len as usize
        }

        fn get_elem_len(elem: &<RichtextTreeTrait as BTreeTrait>::Elem) -> usize {
            match elem {
                RichtextStateChunk::Text {
                    unicode_len: _,
                    text,
                } => count_utf16_chars(text),
                RichtextStateChunk::Style { .. } => 0,
            }
        }

        fn get_offset_and_found(
            left: usize,
            elem: &<RichtextTreeTrait as BTreeTrait>::Elem,
        ) -> (usize, bool) {
            match elem {
                RichtextStateChunk::Text {
                    unicode_len: _,
                    text,
                } => {
                    if left == 0 {
                        return (0, true);
                    }

                    let s = std::str::from_utf8(text).unwrap();
                    let offset = utf16_to_unicode_index(s, left).unwrap();
                    (offset, true)
                }
                RichtextStateChunk::Style { .. } => (1, false),
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
                RichtextStateChunk::Text {
                    unicode_len,
                    text: _,
                } => *unicode_len as usize,
                RichtextStateChunk::Style { .. } => 1,
            }
        }

        fn get_offset_and_found(
            left: usize,
            elem: &<RichtextTreeTrait as BTreeTrait>::Elem,
        ) -> (usize, bool) {
            match elem {
                RichtextStateChunk::Text {
                    unicode_len,
                    text: _,
                } => {
                    if *unicode_len as usize >= left {
                        return (left, true);
                    }

                    (left, false)
                }
                RichtextStateChunk::Style { .. } => {
                    if left == 0 {
                        return (0, true);
                    }

                    (left, false)
                }
            }
        }
    }
}

impl RichtextState {
    pub(crate) fn from_chunks<I: Iterator<Item = impl Into<RichtextStateChunk>>>(i: I) -> Self {
        Self {
            tree: i.collect(),
            style_ranges: Default::default(),
        }
    }

    /// Insert text at a unicode index. Return the entity index.
    pub(crate) fn insert(&mut self, pos: usize, text: BytesSlice) -> usize {
        if self.tree.is_empty() {
            let elem = RichtextStateChunk::try_from_bytes(text).unwrap();
            self.style_ranges.insert(0, elem.rle_len());
            self.tree.push(elem);
            return 0;
        }

        let right = self.find_best_insert_pos::<UnicodeQuery>(pos).unwrap();
        let right = self.tree.prefer_left(right).unwrap_or(right);
        let entity_index = self.get_entity_index_from_path(right);
        let insert_pos = right;
        let elem = RichtextStateChunk::try_from_bytes(text).unwrap();
        self.style_ranges.insert(entity_index, elem.rle_len());
        self.tree.insert_by_path(insert_pos, elem);
        entity_index
    }

    pub(crate) fn get_entity_index_for_text_insert<
        Q: Query<RichtextTreeTrait, QueryArg = usize>,
    >(
        &self,
        pos: usize,
    ) -> usize {
        if self.tree.is_empty() {
            return 0;
        }

        let right = self.find_best_insert_pos::<Q>(pos).unwrap();
        self.get_entity_index_from_path(right)
    }

    /// This is used to accept changes from DiffCalculator
    pub(crate) fn insert_at_entity_index(&mut self, entity_index: usize, text: BytesSlice) {
        let elem = RichtextStateChunk::try_from_bytes(text).unwrap();
        self.style_ranges.insert(entity_index, elem.rle_len());
        self.tree.insert::<EntityQuery>(&entity_index, elem);
    }

    /// This is used to accept changes from DiffCalculator.
    ///
    /// Return event index
    pub(crate) fn insert_elem_at_entity_index(
        &mut self,
        entity_index: usize,
        elem: RichtextStateChunk,
    ) -> (usize, &Styles) {
        debug_assert!(
            entity_index <= self.len_entity(),
            "entity_index={} len={}",
            entity_index,
            self.len_entity()
        );

        match self.tree.query::<EntityQuery>(&entity_index) {
            Some(cursor) => {
                let ans = self.cursor_to_event_index(cursor.cursor);
                let styles = self.style_ranges.insert(entity_index, elem.rle_len());
                self.tree.insert_by_path(cursor.cursor, elem);
                (ans, styles)
            }
            None => {
                let styles = self.style_ranges.insert(entity_index, elem.rle_len());
                self.tree.push(elem);
                (0, styles)
            }
        }
    }

    /// Convert cursor position to event index:
    ///
    /// - If feature="wasm", index is utf16 index,
    /// - If feature!="wasm", index is unicode index,
    pub(crate) fn cursor_to_event_index(&self, cursor: Cursor) -> usize {
        if cfg!(feature = "wasm") {
            let mut ans = 0;
            self.tree
                .visit_previous_caches(cursor, |cache| match cache {
                    generic_btree::PreviousCache::NodeCache(c) => {
                        ans += c.unicode_len as usize;
                    }
                    generic_btree::PreviousCache::PrevSiblingElem(c) => match c {
                        RichtextStateChunk::Text { text, .. } => {
                            ans += count_utf16_chars(text);
                        }
                        RichtextStateChunk::Style { .. } => {}
                    },
                    generic_btree::PreviousCache::ThisElemAndOffset { elem, offset } => {
                        match elem {
                            RichtextStateChunk::Text {
                                unicode_len: _,
                                text,
                            } => {
                                ans += utf16_to_unicode_index(
                                    // SAFETY: we're sure that the text is valid utf8
                                    unsafe { std::str::from_utf8_unchecked(text) },
                                    offset,
                                )
                                .unwrap();
                            }
                            RichtextStateChunk::Style { .. } => {}
                        }
                    }
                });

            ans
        } else {
            let mut ans = 0;
            self.tree
                .visit_previous_caches(cursor, |cache| match cache {
                    generic_btree::PreviousCache::NodeCache(c) => {
                        ans += c.unicode_len;
                    }
                    generic_btree::PreviousCache::PrevSiblingElem(c) => match c {
                        RichtextStateChunk::Text { unicode_len, .. } => {
                            ans += *unicode_len;
                        }
                        RichtextStateChunk::Style { .. } => {}
                    },
                    generic_btree::PreviousCache::ThisElemAndOffset { elem, offset } => {
                        match elem {
                            RichtextStateChunk::Text { .. } => {
                                ans += offset as i32;
                            }
                            RichtextStateChunk::Style { .. } => {}
                        }
                    }
                });
            ans as usize
        }
    }

    /// This method only updates `style_ranges`.
    /// When this method is called, the style start anchor and the style end anchor should already have been inserted.
    pub(crate) fn annotate_style_range(&mut self, range: Range<usize>, style: Arc<StyleOp>) {
        self.style_ranges.annotate(range, style)
    }

    /// Find the best insert position based on algorithm similar to Peritext.
    /// Returns the right neighbor of the insert pos.
    ///
    /// 1. Insertions occur before tombstones that contain the beginning of new marks.
    /// 2. Insertions occur before tombstones that contain the end of bold-like marks
    /// 3. Insertions occur after tombstones that contain the end of link-like marks
    ///
    /// Rule 1 should be satisfied before rules 2 and 3 to avoid this problem.
    ///
    /// The current method will scan forward to find the last position that satisfies 1 and 2.
    /// Then it scans backward to find the first position that satisfies 3.
    fn find_best_insert_pos<Q: Query<RichtextTreeTrait, QueryArg = usize>>(
        &self,
        pos: usize,
    ) -> Option<generic_btree::Cursor> {
        if self.tree.is_empty() {
            return None;
        }

        // There are a range of elements may share the same unicode index
        // because style anchors' lengths are zero in unicode index.

        // Find the start of the range
        let mut iter = if pos == 0 {
            self.tree.start_cursor()
        } else {
            let q = self.tree.query::<Q>(&(pos - 1)).unwrap();
            match self.tree.shift_path_by_one_offset(q.cursor) {
                Some(x) => x,
                // If next is None, we know the range is empty, return directly
                None => return Some(self.tree.end_cursor()),
            }
        };

        // Find the end of the range
        let right = self.tree.query::<Q>(&pos).unwrap().cursor;
        if iter == right {
            // no style anchor between unicode index (pos-1) and (pos)
            return Some(iter);
        }

        // need to scan from left to right
        let mut visited = Vec::new();
        while iter != right {
            let Some(elem) = self.tree.get_elem(iter.leaf) else {
                break;
            };
            let (style, anchor_type) = match elem {
                RichtextStateChunk::Text { .. } => unreachable!(),
                RichtextStateChunk::Style { style, anchor_type } => (style, *anchor_type),
            };

            visited.push((style, anchor_type, iter));
            if anchor_type == AnchorType::Start {
                // case 1. should be before this anchor
                break;
            }

            if style.info.prefer_insert_before(anchor_type) {
                // case 2.
                break;
            }

            iter = match self.tree.shift_path_by_one_offset(iter) {
                Some(x) => x,
                None => self.tree.end_cursor(),
            };
        }

        while let Some((style, anchor_type, top_elem)) = visited.pop() {
            if !style.info.prefer_insert_before(anchor_type) {
                // case 3.
                break;
            }

            iter = top_elem;
        }

        Some(iter)
    }

    fn get_entity_index_from_path(&self, right: generic_btree::Cursor) -> usize {
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

    /// Delete a range of text at the given unicode position.
    ///
    /// Delete a range of text. (The style anchors included in the range are not deleted.)
    pub(crate) fn delete(&mut self, pos: usize, len: usize) -> Vec<Range<usize>> {
        if self.tree.is_empty() {
            return Vec::new();
        }

        let mut style_anchors: Vec<RichtextStateChunk> = Vec::new();
        let mut removed_entity_ranges: Vec<Range<usize>> = Vec::new();
        let q = self.tree.query::<UnicodeQuery>(&pos).unwrap().cursor;
        let mut entity_index = self.get_entity_index_from_path(q);
        let mut deleted = 0;
        // TODO: Delete style anchors whose inner text content is empty

        for span in self.tree.drain_by_query::<UnicodeQuery>(pos..pos + len) {
            match span {
                RichtextStateChunk::Style { .. } => {
                    entity_index += 1;
                    style_anchors.push(span.clone());
                }
                RichtextStateChunk::Text {
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
        self.tree
            .insert_many_by_cursor(q.map(|x| x.cursor), style_anchors);

        removed_entity_ranges
    }

    pub(crate) fn get_text_entity_ranges<Q: Query<RichtextTreeTrait, QueryArg = usize>>(
        &self,
        pos: usize,
        len: usize,
    ) -> Vec<Range<usize>> {
        if self.tree.is_empty() {
            return Vec::new();
        }

        if len == 0 {
            return Vec::new();
        }

        let mut ans: Vec<Range<usize>> = Vec::new();
        let start = self.tree.query::<Q>(&pos).unwrap().cursor;
        let end = self.tree.query::<Q>(&(pos + len)).unwrap().cursor;
        // TODO: assert end cursor is valid
        let mut entity_index = self.get_entity_index_from_path(start);
        for span in self.tree.iter_range(start..end) {
            let start = span.start.unwrap_or(0);
            let end = span.end.unwrap_or(span.elem.rle_len());
            if end == 0 {
                break;
            }

            let len = end - start;
            match span.elem {
                RichtextStateChunk::Text { .. } => {
                    match ans.last_mut() {
                        Some(last) if last.end == entity_index => {
                            last.end += len;
                        }
                        _ => {
                            ans.push(entity_index..entity_index + len);
                        }
                    }
                    entity_index += len;
                }
                RichtextStateChunk::Style { .. } => {
                    entity_index += 1;
                }
            }
        }

        ans
    }

    // PERF: can be splitted into two methods. One is without cursor_to_event_index
    // PERF: can be speed up a lot by detecting whether the range is in a single leaf first
    /// This is used to accept changes from DiffCalculator
    pub(crate) fn drain_by_entity_index(
        &mut self,
        pos: usize,
        len: usize,
    ) -> (impl Iterator<Item = RichtextStateChunk> + '_, usize, usize) {
        assert!(
            pos + len <= self.len_entity(),
            "pos: {}, len: {}, self.len(): {}",
            pos,
            len,
            self.len_entity()
        );
        // FIXME: need to check whether style is removed when its anchors are removed
        self.style_ranges.delete(pos..pos + len);
        let range = pos..pos + len;
        let start = self.tree.query::<EntityQuery>(&range.start);
        let end = self.tree.query::<EntityQuery>(&range.end);
        let start_event_index = self.cursor_to_event_index(start.unwrap().cursor);
        let end_event_index = self.cursor_to_event_index(end.unwrap().cursor);
        (
            generic_btree::iter::Drain::new(&mut self.tree, start, end),
            start_event_index,
            end_event_index,
        )
    }

    #[allow(unused)]
    pub(crate) fn check(&self) {
        self.tree.check();
    }

    pub(crate) fn mark_with_entity_index(&mut self, range: Range<usize>, style: Arc<StyleOp>) {
        if self.tree.is_empty() {
            panic!("Cannot mark an empty tree");
        }

        self.insert_elem_at_entity_index(
            range.end,
            RichtextStateChunk::from_style(style.clone(), AnchorType::End),
        );
        self.insert_elem_at_entity_index(
            range.start,
            RichtextStateChunk::from_style(style.clone(), AnchorType::Start),
        );
        // end_entity_index + 2, because
        // 1. We inserted a start anchor before end_entity_index, so we need to +1
        // 2. We need to include the end anchor in the range, so we need to +1
        self.style_ranges
            .annotate(range.start..range.end + 2, style);
    }

    /// Mark a range of text with a style.
    ///
    /// Return the corresponding entity index ranges.
    pub(crate) fn mark(&mut self, range: Range<usize>, style: Arc<StyleOp>) -> Range<usize> {
        if self.tree.is_empty() {
            panic!("Cannot mark an empty tree");
        }

        let end_pos = self
            .find_best_insert_pos::<UnicodeQuery>(range.end)
            .unwrap();
        let end_entity_index = self.get_entity_index_from_path(end_pos);
        self.tree.insert_by_path(
            end_pos,
            RichtextStateChunk::from_style(style.clone(), AnchorType::End),
        );

        let start_pos = self
            .find_best_insert_pos::<UnicodeQuery>(range.start)
            .unwrap();
        let start_entity_index = self.get_entity_index_from_path(start_pos);
        self.tree.insert_by_path(
            start_pos,
            RichtextStateChunk::from_style(style.clone(), AnchorType::Start),
        );

        self.style_ranges.insert(end_entity_index, 1);
        self.style_ranges.insert(start_entity_index, 1);
        // end_entity_index + 2, because
        // 1. We inserted a start anchor before end_entity_index, so we need to +1
        // 2. We need to include the end anchor in the range, so we need to +1
        self.style_ranges
            .annotate(start_entity_index..end_entity_index + 2, style);

        start_entity_index..end_entity_index
    }

    // FIXME: tests (unstable)
    /// iter item is (event_length, styles)
    pub fn iter_styles_in_event_index_range(
        &self,
        target_event_range: Range<usize>,
    ) -> impl Iterator<Item = (usize, &Styles)> + '_ {
        let start = self
            .tree
            .query::<EventIndexQuery>(&target_event_range.start);
        let start_entity_index = match start {
            Some(start) => self.get_entity_index_from_path(start.cursor),
            None => 0,
        };

        let mut event_index = target_event_range.start;
        let mut entity_index = start_entity_index;
        let mut style_range_iter = self.style_ranges.iter_from(start_entity_index);
        let mut cur_style_range = style_range_iter
            .next()
            .unwrap_or_else(|| (start_entity_index..usize::MAX, &EMPTY_STYLES));
        let mut text_iter = self.tree.iter_range(
            start.map(|x| x.cursor).unwrap_or_else(|| Cursor {
                leaf: self.tree.first_leaf().unwrap_leaf().into(),
                offset: 0,
            })..,
        );
        let mut last_emit_event_index = target_event_range.start;
        std::iter::from_fn(move || loop {
            if entity_index >= cur_style_range.0.end {
                let ans = cur_style_range.1;
                cur_style_range = style_range_iter
                    .next()
                    .unwrap_or_else(|| (entity_index..usize::MAX, &EMPTY_STYLES));
                let len = event_index - last_emit_event_index;
                last_emit_event_index = event_index;
                return Some((len, ans));
            }

            if event_index >= target_event_range.end {
                if last_emit_event_index < target_event_range.end {
                    let ans = cur_style_range.1;
                    let len = target_event_range.end - last_emit_event_index;
                    last_emit_event_index = target_event_range.end;
                    return Some((len, ans));
                } else {
                    return None;
                }
            }

            let Some(slice) = text_iter.next() else {
                if event_index > last_emit_event_index {
                    let ans = cur_style_range.1;
                    let len = event_index - last_emit_event_index;
                    last_emit_event_index = event_index;
                    return Some((len, ans));
                } else {
                    return None;
                }
            };

            let start_offset = slice.start.unwrap_or(0);
            let elem = slice.elem;
            match elem {
                RichtextStateChunk::Text { unicode_len, text } => {
                    event_index += if cfg!(feature = "wasm") {
                        if start_offset == 0 {
                            count_utf16_chars(text)
                        } else {
                            let offset = unicode_to_utf8_index(
                                // SAFETY: we know that the text is valid utf8
                                unsafe { from_utf8_unchecked(text) },
                                start_offset,
                            )
                            .unwrap();
                            count_utf16_chars(&text[offset..])
                        }
                    } else if start_offset == 0 {
                        *unicode_len as usize
                    } else {
                        *unicode_len as usize - start_offset
                    };

                    entity_index += *unicode_len as usize;
                }
                RichtextStateChunk::Style { .. } => {
                    entity_index += 1;
                }
            }
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = RichtextSpan> + '_ {
        let mut entity_index = 0;
        let mut style_range_iter = self.style_ranges.iter();
        let mut cur_style_range = style_range_iter.next();

        fn to_styles(
            (_, style_map): &(Range<usize>, &FxHashMap<InternalString, StyleValue>),
        ) -> Vec<Style> {
            let mut styles = Vec::with_capacity(style_map.len());
            for style in style_map.iter().flat_map(|(_, values)| values.to_styles()) {
                styles.push(style);
            }
            styles
        }

        let mut cur_styles = cur_style_range.as_ref().map(to_styles);

        self.tree.iter().filter_map(move |x| match x {
            RichtextStateChunk::Text { unicode_len, text } => {
                let mut styles = Vec::new();
                while let Some((inner_cur_range, _)) = cur_style_range.as_ref() {
                    if entity_index < inner_cur_range.start {
                        break;
                    }

                    if entity_index < inner_cur_range.end {
                        styles = cur_styles.as_ref().unwrap().clone();
                        break;
                    } else {
                        cur_style_range = style_range_iter.next();
                        cur_styles = cur_style_range.as_ref().map(to_styles);
                    }
                }

                entity_index += *unicode_len as usize;
                Some(RichtextSpan {
                    // SAFETY: We know for sure text is valid utf8
                    text: text.clone().into(),
                    styles,
                })
            }
            RichtextStateChunk::Style { .. } => {
                entity_index += 1;
                None
            }
        })
    }

    pub fn iter_chunk(&self) -> impl Iterator<Item = &RichtextStateChunk> {
        self.tree.iter()
    }

    pub fn get_richtext_value(&self) -> LoroValue {
        let mut ans: Vec<LoroValue> = Vec::new();
        let mut last_style_set: Option<FxHashSet<_>> = None;
        for span in self.iter() {
            let style_set: FxHashSet<Style> = span.styles.iter().cloned().collect();
            if let Some(last) = last_style_set.as_ref() {
                if &style_set == last {
                    let hash_map = ans.last_mut().unwrap().as_map_mut().unwrap();
                    let s = Arc::make_mut(hash_map)
                        .get_mut("insert")
                        .unwrap()
                        .as_string_mut()
                        .unwrap();
                    Arc::make_mut(s).push_str(span.text.as_str());
                    continue;
                }
            }

            let mut value = FxHashMap::default();
            value.insert(
                "insert".into(),
                LoroValue::String(Arc::new(span.text.as_str().into())),
            );

            if !span.styles.is_empty() {
                let mut styles = FxHashMap::default();
                for style in span.styles.iter() {
                    styles.insert(style.key.to_string(), style.data.clone());
                }

                value.insert("attributes".into(), LoroValue::Map(Arc::new(styles)));
            }

            ans.push(LoroValue::Map(Arc::new(value)));
            last_style_set = Some(style_set);
        }

        LoroValue::List(Arc::new(ans))
    }

    pub fn to_vec(&self) -> Vec<RichtextSpan> {
        self.iter().collect()
    }

    #[cfg(test)]
    #[allow(unused)]
    pub(crate) fn debug(&self) {
        dbg!(&self.tree);
        dbg!(&self.style_ranges);
    }

    #[inline(always)]
    pub fn len_unicode(&self) -> usize {
        self.tree.root_cache().unicode_len as usize
    }

    #[inline(always)]
    pub fn len_utf16(&self) -> usize {
        self.tree.root_cache().utf16_len as usize
    }

    #[inline(always)]
    pub fn len_utf8(&self) -> usize {
        self.tree.root_cache().bytes as usize
    }

    #[inline(always)]
    pub fn is_emtpy(&self) -> bool {
        self.tree.root_cache().bytes == 0
    }

    #[inline(always)]
    pub fn len_entity(&self) -> usize {
        self.tree.root_cache().entity_len as usize
    }

    #[inline(always)]
    pub(crate) fn push(&mut self, elem: RichtextStateChunk) {
        self.tree.push(elem);
    }
}

#[cfg(test)]
mod test {
    use append_only_bytes::AppendOnlyBytes;
    use loro_common::{ContainerID, ContainerType, LoroValue, ID};

    use crate::container::richtext::TextStyleInfoFlag;

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

    fn bold(n: isize) -> Arc<StyleOp> {
        Arc::new(StyleOp::new_for_test(n, "bold", TextStyleInfoFlag::BOLD))
    }

    fn comment(n: isize) -> Arc<StyleOp> {
        Arc::new(StyleOp::new_for_test(
            n,
            "comment",
            TextStyleInfoFlag::COMMENT,
        ))
    }

    fn unbold(n: isize) -> Arc<StyleOp> {
        Arc::new(StyleOp::new_for_test(
            n,
            "bold",
            TextStyleInfoFlag::BOLD.to_delete(),
        ))
    }

    fn link(n: isize) -> Arc<StyleOp> {
        Arc::new(StyleOp::new_for_test(n, "link", TextStyleInfoFlag::LINK))
    }

    #[test]
    fn test() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello World!");
        wrapper.state.mark(0..5, bold(0));
        assert_eq!(
            wrapper.state.to_vec(),
            vec![
                RichtextSpan {
                    text: "Hello".into(),
                    styles: vec![Style {
                        key: "bold".into(),
                        data: LoroValue::Bool(true)
                    }]
                },
                RichtextSpan {
                    text: " World!".into(),
                    styles: vec![]
                }
            ]
        );
        wrapper.state.mark(2..7, link(1));
        dbg!(&wrapper.state);
        assert_eq!(
            wrapper.state.to_vec(),
            vec![
                RichtextSpan {
                    text: "He".into(),
                    styles: vec![Style {
                        key: "bold".into(),
                        data: LoroValue::Bool(true)
                    }]
                },
                RichtextSpan {
                    text: "llo".into(),
                    styles: vec![
                        Style {
                            key: "bold".into(),
                            data: LoroValue::Bool(true)
                        },
                        Style {
                            key: "link".into(),
                            data: LoroValue::Bool(true)
                        }
                    ]
                },
                RichtextSpan {
                    text: " W".into(),
                    styles: vec![Style {
                        key: "link".into(),
                        data: LoroValue::Bool(true)
                    }]
                },
                RichtextSpan {
                    text: "orld!".into(),
                    styles: vec![]
                }
            ]
        );
    }

    #[test]
    fn bold_should_expand() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello World!");
        wrapper.state.mark(0..5, bold(0));
        wrapper.insert(5, " Test");
        assert_eq!(
            wrapper.state.to_vec(),
            vec![
                RichtextSpan {
                    text: "Hello".into(),
                    styles: vec![Style {
                        key: "bold".into(),
                        data: LoroValue::Bool(true)
                    }]
                },
                RichtextSpan {
                    text: " Test".into(),
                    styles: vec![Style {
                        key: "bold".into(),
                        data: LoroValue::Bool(true)
                    }]
                },
                RichtextSpan {
                    text: " World!".into(),
                    styles: vec![]
                }
            ]
        );
    }

    #[test]
    fn link_should_not_expand() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello World!");
        wrapper.state.mark(0..5, link(0));
        wrapper.insert(5, " Test");
        assert_eq!(
            wrapper.state.to_vec(),
            vec![
                RichtextSpan {
                    text: "Hello".into(),
                    styles: vec![Style {
                        key: "link".into(),
                        data: LoroValue::Bool(true)
                    }]
                },
                RichtextSpan {
                    text: " Test".into(),
                    styles: vec![]
                },
                RichtextSpan {
                    text: " World!".into(),
                    styles: vec![]
                }
            ]
        );
    }

    #[test]
    fn continuous_text_insert_should_be_merged() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello");
        wrapper.insert(5, " World!");
        assert_eq!(
            wrapper.state.to_vec(),
            vec![RichtextSpan {
                text: "Hello World!".into(),
                styles: vec![]
            },]
        );
    }

    #[test]
    fn continuous_text_insert_should_be_merged_and_have_bold() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello");
        wrapper.state.mark(0..5, bold(0));
        wrapper.insert(5, " World!");
        assert_eq!(
            wrapper.state.to_vec(),
            vec![RichtextSpan {
                text: "Hello World!".into(),
                styles: vec![Style {
                    key: "bold".into(),
                    data: LoroValue::Bool(true)
                }]
            },]
        );
    }

    #[test]
    fn continuous_text_insert_should_not_be_merged_when_prev_is_link() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello");
        wrapper.state.mark(0..5, link(0));
        wrapper.insert(5, " World!");
        assert_eq!(
            wrapper.state.to_vec(),
            vec![
                RichtextSpan {
                    text: "Hello".into(),
                    styles: vec![Style {
                        key: "link".into(),
                        data: LoroValue::Bool(true)
                    },]
                },
                RichtextSpan {
                    text: " World!".into(),
                    styles: vec![]
                },
            ]
        );
    }

    #[test]
    fn delete_bold() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello World!");
        wrapper.state.mark(0..12, bold(0));
        wrapper.state.mark(5..12, unbold(1));
        assert_eq!(
            wrapper.state.to_vec(),
            vec![
                RichtextSpan {
                    text: "Hello".into(),
                    styles: vec![Style {
                        key: "bold".into(),
                        data: LoroValue::Bool(true)
                    }]
                },
                RichtextSpan {
                    text: " World!".into(),
                    styles: vec![]
                }
            ]
        );
        wrapper.insert(5, "A");
        assert_eq!(
            wrapper.state.to_vec(),
            vec![
                RichtextSpan {
                    text: "Hello".into(),
                    styles: vec![Style {
                        key: "bold".into(),
                        data: LoroValue::Bool(true)
                    }]
                },
                RichtextSpan {
                    text: "A".into(),
                    styles: vec![Style {
                        key: "bold".into(),
                        data: LoroValue::Bool(true)
                    }]
                },
                RichtextSpan {
                    text: " World!".into(),
                    styles: vec![]
                }
            ]
        );

        wrapper.insert(0, "A");
        assert_eq!(
            wrapper.state.to_vec(),
            vec![
                RichtextSpan {
                    text: "A".into(),
                    styles: vec![]
                },
                RichtextSpan {
                    text: "Hello".into(),
                    styles: vec![Style {
                        key: "bold".into(),
                        data: LoroValue::Bool(true)
                    }]
                },
                RichtextSpan {
                    text: "A".into(),
                    styles: vec![Style {
                        key: "bold".into(),
                        data: LoroValue::Bool(true)
                    }]
                },
                RichtextSpan {
                    text: " World!".into(),
                    styles: vec![]
                }
            ]
        );
    }

    #[test]
    fn bold_and_link_at_the_same_place() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello");
        wrapper.state.mark(0..5, link(0));
        wrapper.state.mark(0..5, bold(1));
        wrapper.insert(5, "A");

        assert_eq!(
            wrapper.state.to_vec(),
            vec![
                RichtextSpan {
                    text: "Hello".into(),
                    styles: vec![
                        Style {
                            key: "bold".into(),
                            data: LoroValue::Bool(true)
                        },
                        Style {
                            key: "link".into(),
                            data: LoroValue::Bool(true)
                        }
                    ]
                },
                RichtextSpan {
                    text: "A".into(),
                    styles: vec![Style {
                        key: "bold".into(),
                        data: LoroValue::Bool(true)
                    }]
                },
            ]
        );
    }

    #[test]
    fn comments() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello World!");
        wrapper.state.mark(0..5, comment(0));
        wrapper.state.mark(1..6, comment(1));
        assert_eq!(
            wrapper.state.to_vec(),
            vec![
                RichtextSpan {
                    text: "H".into(),
                    styles: vec![Style {
                        key: "comment".into(),
                        data: LoroValue::Container(ContainerID::new_normal(
                            ID::new(0, 0),
                            ContainerType::Map
                        ))
                    },]
                },
                RichtextSpan {
                    text: "ello".into(),
                    styles: vec![
                        Style {
                            key: "comment".into(),
                            data: LoroValue::Container(ContainerID::new_normal(
                                ID::new(0, 0),
                                ContainerType::Map
                            ))
                        },
                        Style {
                            key: "comment".into(),
                            data: LoroValue::Container(ContainerID::new_normal(
                                ID::new(1, 1),
                                ContainerType::Map
                            ))
                        },
                    ]
                },
                RichtextSpan {
                    text: " ".into(),
                    styles: vec![Style {
                        key: "comment".into(),
                        data: LoroValue::Container(ContainerID::new_normal(
                            ID::new(1, 1),
                            ContainerType::Map
                        ))
                    },]
                },
                RichtextSpan {
                    text: "World!".into(),
                    styles: vec![]
                },
            ]
        );
    }

    #[test]
    fn remove_style_anchors_should_also_delete_style() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello World!");
        wrapper.state.mark(0..5, bold(0));
        let mut count = 0;
        for span in wrapper.state.drain_by_entity_index(0, 7).0 {
            if matches!(span, RichtextStateChunk::Style { .. }) {
                count += 1;
            }
        }

        assert_eq!(count, 2);
        assert_eq!(
            wrapper.state.to_vec(),
            vec![RichtextSpan {
                text: " World!".into(),
                styles: vec![]
            },]
        );
    }
}
