use append_only_bytes::BytesSlice;
use fxhash::FxHashMap;
use generic_btree::{
    rle::{HasLength, Mergeable, Sliceable},
    BTree, BTreeTrait, Cursor,
};
use loro_common::LoroValue;
use serde::{ser::SerializeStruct, Serialize};
use std::fmt::{Display, Formatter};
use std::{
    ops::{Add, AddAssign, Range, Sub},
    str::Utf8Error,
    sync::Arc,
};

use crate::{
    container::richtext::query_by_len::{
        EntityIndexQueryWithEventIndex, IndexQueryWithEntityIndex,
    },
    delta::{DeltaValue, Meta, StyleMeta},
};

// FIXME: Check splice and other things are using unicode index
use self::{
    cursor_cache::CursorCache,
    query::{
        EntityQuery, EntityQueryT, EventIndexQuery, EventIndexQueryT, UnicodeQuery, UnicodeQueryT,
        Utf16Query, Utf16QueryT,
    },
};

use super::{
    query_by_len::{IndexQuery, QueryByLen},
    style_range_map::{StyleRangeMap, Styles},
    AnchorType, RichtextSpan, StyleOp,
};

pub(crate) use query::PosType;

#[derive(Clone, Debug, Default)]
pub(crate) struct RichtextState {
    tree: BTree<RichtextTreeTrait>,
    style_ranges: StyleRangeMap,
    cursor_cache: CursorCache,
}

impl Display for RichtextState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for span in self.tree.iter() {
            match span {
                RichtextStateChunk::Style { .. } => {}
                RichtextStateChunk::Text(s) => {
                    f.write_str(s.as_str())?;
                }
            }
        }

        Ok(())
    }
}

pub(crate) use text_chunk::TextChunk;
mod text_chunk {
    use std::ops::Range;

    use append_only_bytes::BytesSlice;

    #[derive(Clone, Debug)]
    pub(crate) struct TextChunk {
        unicode_len: i32,
        bytes: BytesSlice,
        // TODO: make this field only available in wasm mode
        utf16_len: i32,
    }

    impl TextChunk {
        pub fn from_bytes(bytes: BytesSlice) -> Self {
            let mut utf16_len = 0;
            let mut unicode_len = 0;
            for c in std::str::from_utf8(&bytes).unwrap().chars() {
                utf16_len += c.len_utf16();
                unicode_len += 1;
            }

            Self {
                unicode_len,
                bytes,
                utf16_len: utf16_len as i32,
            }
        }

        pub fn bytes(&self) -> &BytesSlice {
            &self.bytes
        }

        pub fn as_str(&self) -> &str {
            // SAFETY: We know that the text is valid UTF-8
            unsafe { std::str::from_utf8_unchecked(&self.bytes) }
        }

        pub fn len(&self) -> i32 {
            self.unicode_len
        }

        pub fn unicode_len(&self) -> i32 {
            self.unicode_len
        }

        pub fn utf16_len(&self) -> i32 {
            self.utf16_len
        }

        pub fn event_len(&self) -> i32 {
            if cfg!(feature = "wasm") {
                self.utf16_len
            } else {
                self.unicode_len
            }
        }

        /// Convert a unicode index on this text to an event index
        pub fn convert_unicode_offset_to_event_offset(&self, offset: usize) -> usize {
            if cfg!(feature = "wasm") {
                let mut event_offset = 0;
                for (i, c) in self.as_str().chars().enumerate() {
                    if i == offset {
                        return event_offset;
                    }
                    event_offset += c.len_utf16();
                }
                event_offset
            } else {
                offset
            }
        }

        pub fn new_empty() -> Self {
            Self {
                unicode_len: 0,
                bytes: BytesSlice::empty(),
                utf16_len: 0,
            }
        }

        pub(crate) fn delete_by_entity_index(
            &mut self,
            unicode_offset: usize,
            unicode_len: usize,
        ) -> (Option<Self>, usize) {
            let s = self.as_str();
            let mut start_byte = 0;
            let mut end_byte = self.bytes().len();
            let start_unicode_index = unicode_offset;
            let end_unicode_index = unicode_offset + unicode_len;
            let mut start_utf16_index = 0;
            let mut current_utf16_index = 0;
            let mut current_utf8_index = 0;
            for (current_unicode_index, c) in s.chars().enumerate() {
                if current_unicode_index == start_unicode_index {
                    start_utf16_index = current_utf16_index;
                    start_byte = current_utf8_index;
                }

                if current_unicode_index == end_unicode_index {
                    end_byte = current_utf8_index;
                    break;
                }

                current_utf16_index += c.len_utf16();
                current_utf8_index += c.len_utf8();
            }

            self.utf16_len -= (current_utf16_index - start_utf16_index) as i32;

            let event_len = if cfg!(feature = "wasm") {
                current_utf16_index - start_utf16_index
            } else {
                unicode_len
            };

            self.unicode_len -= unicode_len as i32;
            let next = match (start_byte == 0, end_byte == self.bytes.len()) {
                (true, true) => {
                    self.bytes = BytesSlice::empty();
                    None
                }
                (true, false) => {
                    self.bytes.slice_(end_byte..);
                    None
                }
                (false, true) => {
                    self.bytes.slice_(..start_byte);
                    None
                }
                (false, false) => {
                    let next = self.bytes.slice_clone(end_byte..);
                    let next = Self::from_bytes(next);
                    self.unicode_len -= next.unicode_len;
                    self.utf16_len -= next.utf16_len;
                    self.bytes.slice_(..start_byte);
                    Some(next)
                }
            };

            self.check();
            if let Some(next) = next.as_ref() {
                next.check();
            }
            (next, event_len)
        }

        fn check(&self) {
            if cfg!(debug_assertions) {
                assert_eq!(self.unicode_len, self.as_str().chars().count() as i32);
                assert_eq!(
                    self.utf16_len,
                    self.as_str().chars().map(|c| c.len_utf16()).sum::<usize>() as i32
                );
            }
        }
    }

    impl generic_btree::rle::HasLength for TextChunk {
        fn rle_len(&self) -> usize {
            self.unicode_len as usize
        }
    }

    impl generic_btree::rle::Sliceable for TextChunk {
        fn _slice(&self, range: Range<usize>) -> Self {
            assert!(range.start < range.end);
            let mut utf16_len = 0;
            let mut start = 0;
            let mut end = 0;
            let mut started = false;
            for (unicode_index, (i, c)) in self.as_str().char_indices().enumerate() {
                if unicode_index == range.start {
                    start = i;
                    started = true;
                }
                if unicode_index == range.end {
                    end = i;
                    break;
                }
                if started {
                    utf16_len += c.len_utf16();
                }
            }

            let ans = Self {
                unicode_len: range.len() as i32,
                bytes: self.bytes.slice_clone(start..end),
                utf16_len: utf16_len as i32,
            };
            ans.check();
            ans
        }

        fn split(&mut self, pos: usize) -> Self {
            let mut utf16_len = 0;
            let mut byte_offset = 0;
            for (unicode_index, (i, c)) in self.as_str().char_indices().enumerate() {
                if unicode_index == pos {
                    byte_offset = i;
                    break;
                }

                utf16_len += c.len_utf16();
            }
            let right = Self {
                unicode_len: self.unicode_len - pos as i32,
                bytes: self.bytes.slice_clone(byte_offset..),
                utf16_len: self.utf16_len - utf16_len as i32,
            };

            self.unicode_len = pos as i32;
            self.utf16_len = utf16_len as i32;
            self.bytes.slice_(..byte_offset);
            right.check();
            self.check();
            right
        }
    }

    impl generic_btree::rle::Mergeable for TextChunk {
        fn can_merge(&self, rhs: &Self) -> bool {
            self.bytes.can_merge(&rhs.bytes)
        }

        fn merge_right(&mut self, rhs: &Self) {
            self.bytes.try_merge(&rhs.bytes).unwrap();
            self.utf16_len += rhs.utf16_len;
            self.unicode_len += rhs.unicode_len;
            self.check();
        }

        fn merge_left(&mut self, left: &Self) {
            let mut new = left.bytes.clone();
            new.try_merge(&self.bytes).unwrap();
            self.bytes = new;
            self.utf16_len += left.utf16_len;
            self.unicode_len += left.unicode_len;
            self.check();
        }
    }
}

// TODO: change visibility back to crate after #116 is done
#[derive(Clone, Debug)]
pub(crate) enum RichtextStateChunk {
    Text(TextChunk),
    Style {
        style: Arc<StyleOp>,
        anchor_type: AnchorType,
    },
}

impl RichtextStateChunk {
    pub fn new_text(s: BytesSlice) -> Self {
        Self::Text(TextChunk::from_bytes(s))
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
            RichtextStateChunk::Text(text) => {
                let mut state = serializer.serialize_struct("RichtextStateChunk", 3)?;
                state.serialize_field("type", "Text")?;
                state.serialize_field("unicode_len", &text.unicode_len())?;
                state.serialize_field("text", text.as_str())?;
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
        std::str::from_utf8(&s)?;
        Ok(RichtextStateChunk::Text(TextChunk::from_bytes(s)))
    }

    pub fn from_style(style: Arc<StyleOp>, anchor_type: AnchorType) -> Self {
        Self::Style { style, anchor_type }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            RichtextStateChunk::Text(text) => Some(text.as_str()),
            _ => None,
        }
    }
}

impl HasLength for RichtextStateChunk {
    fn rle_len(&self) -> usize {
        match self {
            RichtextStateChunk::Text(s) => s.rle_len(),
            RichtextStateChunk::Style { .. } => 1,
        }
    }
}

impl Mergeable for RichtextStateChunk {
    fn can_merge(&self, rhs: &Self) -> bool {
        match (self, rhs) {
            (RichtextStateChunk::Text(l), RichtextStateChunk::Text(r)) => l.can_merge(r),
            _ => false,
        }
    }

    fn merge_right(&mut self, rhs: &Self) {
        match (self, rhs) {
            (RichtextStateChunk::Text(l), RichtextStateChunk::Text(r)) => l.merge_right(r),
            _ => unreachable!(),
        }
    }

    fn merge_left(&mut self, left: &Self) {
        match (self, left) {
            (RichtextStateChunk::Text(this), RichtextStateChunk::Text(left)) => {
                this.merge_left(left)
            }
            _ => unreachable!(),
        }
    }
}

impl Sliceable for RichtextStateChunk {
    fn _slice(&self, range: Range<usize>) -> Self {
        match self {
            RichtextStateChunk::Text(s) => RichtextStateChunk::Text(s._slice(range)),
            RichtextStateChunk::Style { style, anchor_type } => {
                assert_eq!(range.start, 0);
                assert_eq!(range.end, 1);
                RichtextStateChunk::Style {
                    style: style.clone(),
                    anchor_type: *anchor_type,
                }
            }
        }
    }

    fn split(&mut self, pos: usize) -> Self {
        match self {
            RichtextStateChunk::Text(s) => RichtextStateChunk::Text(s.split(pos)),
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

pub(crate) fn unicode_to_utf16_index(s: &str, unicode_index: usize) -> Option<usize> {
    if unicode_index == 0 {
        return Some(0);
    }

    let mut current_unicode_index = 0;
    let mut current_utf16_index = 0;
    for c in s.chars() {
        let len = c.len_utf16();
        current_unicode_index += 1;
        current_utf16_index += len;
        if current_unicode_index == unicode_index {
            return Some(current_utf16_index);
        }
    }

    None
}

pub(crate) fn utf16_to_utf8_index(s: &str, utf16_index: usize) -> Option<usize> {
    if utf16_index == 0 {
        return Some(0);
    }

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

/// Returns the unicode index of the character at the given utf16 index.
///
/// If the given utf16 index is not at the correct boundary, returns the unicode index of the
/// character before the given utf16 index.
pub(crate) fn utf16_to_unicode_index(s: &str, utf16_index: usize) -> Result<usize, usize> {
    if utf16_index == 0 {
        return Ok(0);
    }

    let mut current_utf16_index = 0;
    let mut current_unicode_index = 0;
    for (i, c) in s.chars().enumerate() {
        let len = c.len_utf16();
        current_utf16_index += len;
        if current_utf16_index == utf16_index {
            return Ok(i + 1);
        }
        if current_utf16_index > utf16_index {
            debug_log::debug_log!("WARNING: UTF16 MISMATCHED!");
            return Err(i);
        }
        current_unicode_index = i + 1;
    }

    debug_log::debug_log!("WARNING: UTF16 MISMATCHED!");
    Err(current_unicode_index)
}

fn pos_to_unicode_index(s: &str, pos: usize, kind: PosType) -> Option<usize> {
    match kind {
        PosType::Bytes => todo!(),
        PosType::Unicode => Some(pos),
        PosType::Utf16 => utf16_to_unicode_index(s, pos).ok(),
        PosType::Entity => Some(pos),
        PosType::Event => {
            if cfg!(feature = "wasm") {
                utf16_to_unicode_index(s, pos).ok()
            } else {
                Some(pos)
            }
        }
    }
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Default)]
pub(crate) struct PosCache {
    pub(super) unicode_len: i32,
    pub(super) bytes: i32,
    pub(super) utf16_len: i32,
    pub(super) entity_len: i32,
}

impl PosCache {
    pub(crate) fn event_len(&self) -> i32 {
        if cfg!(feature = "wasm") {
            self.utf16_len
        } else {
            self.unicode_len
        }
    }

    #[allow(unused)]
    fn get_len(&self, pos_type: PosType) -> i32 {
        match pos_type {
            PosType::Bytes => self.bytes,
            PosType::Unicode => self.unicode_len,
            PosType::Utf16 => self.utf16_len,
            PosType::Entity => self.entity_len,
            PosType::Event => self.event_len(),
        }
    }
}

impl AddAssign for PosCache {
    fn add_assign(&mut self, rhs: Self) {
        self.unicode_len += rhs.unicode_len;
        self.bytes += rhs.bytes;
        self.utf16_len += rhs.utf16_len;
        self.entity_len += rhs.entity_len;
    }
}

impl Add for PosCache {
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

impl Sub for PosCache {
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

    type Cache = PosCache;

    type CacheDiff = PosCache;

    fn calc_cache_internal(
        cache: &mut Self::Cache,
        caches: &[generic_btree::Child<Self>],
    ) -> Self::CacheDiff {
        let mut new_cache = PosCache::default();
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
            RichtextStateChunk::Text(s) => PosCache {
                bytes: s.bytes().len() as i32,
                unicode_len: s.unicode_len(),
                utf16_len: s.utf16_len(),
                entity_len: s.unicode_len(),
            },
            RichtextStateChunk::Style { .. } => PosCache {
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
        PosCache {
            bytes: cache_lhs.bytes - cache_rhs.bytes,
            unicode_len: cache_lhs.unicode_len - cache_rhs.unicode_len,
            utf16_len: cache_lhs.utf16_len - cache_rhs.utf16_len,
            entity_len: cache_lhs.entity_len - cache_rhs.entity_len,
        }
    }
}

// This query implementation will prefer right element when both left element and right element are valid.
mod query {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum PosType {
        #[allow(unused)]
        Bytes,
        #[allow(unused)]
        Unicode,
        #[allow(unused)]
        Utf16,
        Entity,
        Event,
    }

    #[cfg(not(feature = "wasm"))]
    pub(super) type EventIndexQuery = UnicodeQuery;
    #[cfg(feature = "wasm")]
    pub(super) type EventIndexQuery = Utf16Query;

    #[cfg(not(feature = "wasm"))]
    pub(super) type EventIndexQueryT = UnicodeQueryT;
    #[cfg(feature = "wasm")]
    pub(super) type EventIndexQueryT = Utf16QueryT;

    pub(super) struct UnicodeQueryT;
    pub(super) type UnicodeQuery = IndexQuery<UnicodeQueryT, RichtextTreeTrait>;

    impl QueryByLen<RichtextTreeTrait> for UnicodeQueryT {
        fn get_cache_len(cache: &<RichtextTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.unicode_len as usize
        }

        fn get_elem_len(elem: &<RichtextTreeTrait as BTreeTrait>::Elem) -> usize {
            match elem {
                RichtextStateChunk::Text(s) => s.rle_len(),
                RichtextStateChunk::Style { .. } => 0,
            }
        }

        fn get_offset_and_found(
            left: usize,
            elem: &<RichtextTreeTrait as BTreeTrait>::Elem,
        ) -> (usize, bool) {
            match elem {
                RichtextStateChunk::Text(s) => {
                    if s.rle_len() >= left {
                        return (left, true);
                    }

                    (left, false)
                }
                RichtextStateChunk::Style { .. } => (1, false),
            }
        }

        fn get_cache_entity_len(cache: &<RichtextTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.entity_len as usize
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
                RichtextStateChunk::Text(s) => s.utf16_len() as usize,
                RichtextStateChunk::Style { .. } => 0,
            }
        }

        fn get_offset_and_found(
            left: usize,
            elem: &<RichtextTreeTrait as BTreeTrait>::Elem,
        ) -> (usize, bool) {
            match elem {
                RichtextStateChunk::Text(s) => {
                    if left == 0 {
                        return (0, true);
                    }

                    // Allow left to not at the correct utf16 boundary. If so fallback to the last position.
                    // TODO: if we remove the use of query(pos-1), we won't need this fallback behaviro
                    let offset = utf16_to_unicode_index(s.as_str(), left).unwrap_or_else(|e| e);
                    (offset, true)
                }
                RichtextStateChunk::Style { .. } => (1, false),
            }
        }

        fn get_cache_entity_len(cache: &<RichtextTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.entity_len as usize
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
                RichtextStateChunk::Text(s) => s.rle_len(),
                RichtextStateChunk::Style { .. } => 1,
            }
        }

        fn get_offset_and_found(
            left: usize,
            elem: &<RichtextTreeTrait as BTreeTrait>::Elem,
        ) -> (usize, bool) {
            match elem {
                RichtextStateChunk::Text(s) => {
                    if s.rle_len() >= left {
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

        fn get_cache_entity_len(cache: &<RichtextTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.entity_len as usize
        }
    }
}

mod cursor_cache {
    use std::sync::atomic::AtomicUsize;

    use super::{pos_to_unicode_index, unicode_to_utf16_index, PosType, RichtextTreeTrait};
    use generic_btree::{rle::HasLength, BTree, Cursor, LeafIndex};

    #[derive(Debug, Clone)]
    struct CursorCacheItem {
        pos: usize,
        pos_type: PosType,
        leaf: LeafIndex,
    }

    #[derive(Debug, Clone)]
    struct EntityIndexCacheItem {
        pos: usize,
        pos_type: PosType,
        entity_index: usize,
        leaf: LeafIndex,
    }

    #[derive(Debug, Clone, Default)]
    pub(super) struct CursorCache {
        cursor: Option<CursorCacheItem>,
        entity: Option<EntityIndexCacheItem>,
    }

    static CACHE_HIT: AtomicUsize = AtomicUsize::new(0);
    static CACHE_MISS: AtomicUsize = AtomicUsize::new(0);

    impl CursorCache {
        // TODO: some of the invalidation can be changed into shifting pos
        pub fn invalidate(&mut self) {
            self.cursor.take();
            self.entity.take();
        }

        pub fn invalidate_entity_cache_after(&mut self, entity_index: usize) {
            if let Some(c) = self.entity.as_mut() {
                if entity_index < c.entity_index {
                    self.entity = None;
                }
            }
        }

        pub fn record_cursor(
            &mut self,
            pos: usize,
            kind: PosType,
            cursor: Cursor,
            _tree: &BTree<RichtextTreeTrait>,
        ) {
            match kind {
                PosType::Unicode | PosType::Entity => {
                    self.cursor = Some(CursorCacheItem {
                        pos: pos - cursor.offset,
                        pos_type: kind,
                        leaf: cursor.leaf,
                    });
                }
                PosType::Utf16 => todo!(),
                PosType::Event => todo!(),
                PosType::Bytes => todo!(),
            }
        }

        pub fn record_entity_index(
            &mut self,
            pos: usize,
            kind: PosType,
            entity_index: usize,
            cursor: Cursor,
            tree: &BTree<RichtextTreeTrait>,
        ) {
            match kind {
                PosType::Bytes => todo!(),
                PosType::Unicode | PosType::Entity => {
                    self.entity = Some(EntityIndexCacheItem {
                        pos: pos - cursor.offset,
                        pos_type: kind,
                        entity_index: entity_index - cursor.offset,
                        leaf: cursor.leaf,
                    });
                }
                PosType::Event if cfg!(not(feature = "wasm")) => {
                    self.entity = Some(EntityIndexCacheItem {
                        pos: pos - cursor.offset,
                        pos_type: kind,
                        entity_index: entity_index - cursor.offset,
                        leaf: cursor.leaf,
                    });
                }
                _ => {
                    // utf16
                    if cursor.offset == 0 {
                        self.entity = Some(EntityIndexCacheItem {
                            pos,
                            pos_type: kind,
                            entity_index,
                            leaf: cursor.leaf,
                        });
                    } else {
                        let elem = tree.get_elem(cursor.leaf).unwrap();
                        let Some(s) = elem.as_str() else { return };
                        let utf16offset = unicode_to_utf16_index(s, cursor.offset).unwrap();
                        self.entity = Some(EntityIndexCacheItem {
                            pos: pos - utf16offset,
                            pos_type: kind,
                            entity_index: entity_index - cursor.offset,
                            leaf: cursor.leaf,
                        });
                    }
                }
            }
        }

        pub fn get_cursor(
            &self,
            pos: usize,
            pos_type: PosType,
            tree: &BTree<RichtextTreeTrait>,
        ) -> Option<Cursor> {
            for c in self.cursor.iter() {
                if c.pos_type != pos_type {
                    continue;
                }

                let elem = tree.get_elem(c.leaf).unwrap();
                let Some(s) = elem.as_str() else { continue };
                if pos < c.pos {
                    continue;
                }

                let offset = pos - c.pos;
                let Some(offset) = pos_to_unicode_index(s, offset, pos_type) else {
                    continue;
                };

                if offset <= elem.rle_len() {
                    cache_hit();
                    return Some(Cursor {
                        leaf: c.leaf,
                        offset,
                    });
                }
            }

            cache_miss();
            None
        }

        pub fn get_entity_index(
            &self,
            pos: usize,
            pos_type: PosType,
            tree: &BTree<RichtextTreeTrait>,
            has_style: bool,
        ) -> Option<usize> {
            if has_style {
                return None;
            }

            for c in self.entity.iter() {
                if c.pos_type != pos_type {
                    continue;
                }
                if pos < c.pos {
                    continue;
                }

                let offset = pos - c.pos;
                let leaf = tree.get_leaf(c.leaf.into());
                let Some(s) = leaf.elem().as_str() else {
                    return None;
                };

                let Some(offset) = pos_to_unicode_index(s, offset, pos_type) else {
                    continue;
                };

                if offset < leaf.elem().rle_len() {
                    cache_hit();
                    return Some(offset + c.entity_index);
                }

                cache_hit();
                return Some(offset + c.entity_index);
            }

            cache_miss();
            None
        }

        pub fn diagnose() {
            let hit = CACHE_HIT.load(std::sync::atomic::Ordering::Relaxed);
            let miss = CACHE_MISS.load(std::sync::atomic::Ordering::Relaxed);
            println!(
                "hit: {}, miss: {}, hit rate: {}",
                hit,
                miss,
                hit as f64 / (hit + miss) as f64
            );
        }
    }

    fn cache_hit() {
        #[cfg(debug_assertions)]
        {
            CACHE_HIT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn cache_miss() {
        #[cfg(debug_assertions)]
        {
            CACHE_MISS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

impl RichtextState {
    pub(crate) fn from_chunks<I: Iterator<Item = impl Into<RichtextStateChunk>>>(i: I) -> Self {
        Self {
            tree: i.collect(),
            style_ranges: Default::default(),
            cursor_cache: Default::default(),
        }
    }

    pub(crate) fn get_entity_index_for_text_insert(
        &mut self,
        pos: usize,
        pos_type: PosType,
    ) -> usize {
        if self.tree.is_empty() {
            return 0;
        }

        if let Some(pos) = self.cursor_cache.get_entity_index(
            pos,
            pos_type,
            &self.tree,
            self.style_ranges.has_style(),
        ) {
            debug_assert!(
                pos <= self.len_entity(),
                "tree:{:#?}\ncache:{:#?}",
                &self.tree,
                &self.cursor_cache
            );
            return pos;
        }

        let (c, entity_index) = match pos_type {
            PosType::Bytes => todo!(),
            PosType::Unicode => self.find_best_insert_pos::<UnicodeQueryT>(pos),
            PosType::Utf16 => self.find_best_insert_pos::<Utf16QueryT>(pos),
            PosType::Entity => self.find_best_insert_pos::<EntityQueryT>(pos),
            PosType::Event => self.find_best_insert_pos::<EventIndexQueryT>(pos),
        };

        if let Some(c) = c {
            self.cursor_cache
                .record_cursor(entity_index, PosType::Entity, c, &self.tree);
            if !self.style_ranges.has_style() {
                self.cursor_cache
                    .record_entity_index(pos, pos_type, entity_index, c, &self.tree);
            }
        }

        entity_index
    }

    /// Get the insert text styles at the given entity index if we insert text at that position
    ///
    // TODO: PERF we can avoid this calculation by getting it when inserting new text
    // but that requires a lot of changes
    pub(crate) fn get_styles_at_entity_index_for_insert(
        &mut self,
        entity_index: usize,
    ) -> StyleMeta {
        if !self.style_ranges.has_style() {
            return Default::default();
        }

        self.style_ranges.get_styles_for_insert(entity_index)
    }

    /// This is used to accept changes from DiffCalculator
    pub(crate) fn insert_at_entity_index(&mut self, entity_index: usize, text: BytesSlice) {
        let elem = RichtextStateChunk::try_from_bytes(text).unwrap();
        self.style_ranges.insert(entity_index, elem.rle_len());
        let leaf;
        if let Some(cursor) =
            self.cursor_cache
                .get_cursor(entity_index, PosType::Entity, &self.tree)
        {
            let p = self.tree.prefer_left(cursor).unwrap_or(cursor);
            leaf = self.tree.insert_by_path(p, elem).0;
        } else {
            leaf = {
                let q = &entity_index;
                match self.tree.query::<EntityQuery>(q) {
                    Some(result) => {
                        let p = self
                            .tree
                            .prefer_left(result.cursor)
                            .unwrap_or(result.cursor);
                        self.tree.insert_by_path(p, elem).0
                    }
                    None => self.tree.push(elem),
                }
            };
        }

        self.cursor_cache
            .invalidate_entity_cache_after(entity_index);
        self.cursor_cache
            .record_cursor(entity_index, PosType::Entity, leaf, &self.tree);
    }

    /// This is used to accept changes from DiffCalculator.
    ///
    /// Return (event_index, styles)
    pub(crate) fn insert_elem_at_entity_index(
        &mut self,
        entity_index: usize,
        elem: RichtextStateChunk,
    ) -> (usize, &Styles) {
        debug_assert!(
            entity_index <= self.len_entity(),
            "entity_index={} len={} self={:#?}",
            entity_index,
            self.len_entity(),
            &self
        );

        let cursor;
        let event_index;
        if let Some(cached_cursor) =
            self.cursor_cache
                .get_cursor(entity_index, PosType::Entity, &self.tree)
        {
            cursor = Some(cached_cursor);
            // PERF: how can we avoid this convert
            event_index = self.cursor_to_event_index(cached_cursor);
        } else {
            let (c, f) = self
                .tree
                .query_with_finder_return::<EntityIndexQueryWithEventIndex>(&entity_index);
            cursor = c.map(|x| x.cursor);
            event_index = f.event_index;
        }

        self.cursor_cache.invalidate();
        match cursor {
            Some(cursor) => {
                let styles = self.style_ranges.insert(entity_index, elem.rle_len());
                let cursor = self.tree.insert_by_path(cursor, elem).0;
                self.cursor_cache
                    .record_cursor(entity_index, PosType::Entity, cursor, &self.tree);
                (event_index, styles)
            }
            None => {
                let styles = self.style_ranges.insert(entity_index, elem.rle_len());
                let cursor = self.tree.push(elem);
                self.cursor_cache
                    .record_cursor(entity_index, PosType::Entity, cursor, &self.tree);
                (0, styles)
            }
        }
    }

    /// Convert cursor position to event index:
    ///
    /// - If feature="wasm", index is utf16 index,
    /// - If feature!="wasm", index is unicode index,
    ///
    // PERF: this is slow
    pub(crate) fn cursor_to_event_index(&self, cursor: Cursor) -> usize {
        if cfg!(feature = "wasm") {
            let mut ans = 0;
            self.tree
                .visit_previous_caches(cursor, |cache| match cache {
                    generic_btree::PreviousCache::NodeCache(c) => {
                        ans += c.utf16_len as usize;
                    }
                    generic_btree::PreviousCache::PrevSiblingElem(c) => match c {
                        RichtextStateChunk::Text(s) => {
                            ans += s.utf16_len() as usize;
                        }
                        RichtextStateChunk::Style { .. } => {}
                    },
                    generic_btree::PreviousCache::ThisElemAndOffset { elem, offset } => {
                        match elem {
                            RichtextStateChunk::Text(s) => {
                                ans += s.convert_unicode_offset_to_event_offset(offset);
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
                        RichtextStateChunk::Text(s) => {
                            ans += s.unicode_len();
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
    /// The result is only different from `query` when there are style anchors around the insert pos.
    /// Returns the right neighbor of the insert pos and the entity index.
    ///
    /// 1. Insertions occur before tombstones that contain the beginning of new marks.
    /// 2. Insertions occur before tombstones that contain the end of bold-like marks
    /// 3. Insertions occur after tombstones that contain the end of link-like marks
    ///
    /// Rule 1 should be satisfied before rules 2 and 3 to avoid this problem.
    ///
    /// The current method will scan forward to find the last position that satisfies 1 and 2.
    /// Then it scans backward to find the first position that satisfies 3.
    fn find_best_insert_pos<Q: QueryByLen<RichtextTreeTrait>>(
        &self,
        pos: usize,
    ) -> (Option<generic_btree::Cursor>, usize) {
        type Query<Q> = IndexQueryWithEntityIndex<Q, RichtextTreeTrait>;
        if self.tree.is_empty() {
            return (None, 0);
        }

        // There are a range of elements may share the same unicode index
        // because style anchors' lengths are zero in unicode index.

        // Find the start and the end of the range, and entity index of left cursor
        let (left, right, mut entity_index) = if pos == 0 {
            let left = self.tree.start_cursor();
            let mut right = left;
            let mut elem = self.tree.get_elem(right.leaf).unwrap();
            let entity_index = 0;
            if matches!(elem, RichtextStateChunk::Text { .. }) {
                return (Some(right), 0);
            } else {
                while Q::get_elem_len(elem) == 0 {
                    match self.tree.next_elem(right) {
                        Some(r) => {
                            right = r;
                            elem = self.tree.get_elem(right.leaf).unwrap();
                        }
                        None => {
                            right.offset = elem.rle_len();
                            break;
                        }
                    }
                }

                (left, right, entity_index)
            }
        } else {
            // The query perfers right when there are empty elements (style anchors)
            // So the nodes between (pos-1) and pos are all style anchors.
            let (q, f) = self.tree.query_with_finder_return::<Query<Q>>(&(pos - 1));
            let q = q.unwrap();
            let mut entity_index = f.entity_index();

            let elem = self.tree.get_elem(q.leaf()).unwrap();
            let mut right = q.cursor;
            right.offset += 1;
            entity_index += 1;
            if elem.rle_len() > right.offset {
                // The cursor is in the middle of a style anchor
                return (Some(right), entity_index);
            }

            match self.tree.next_elem(q.cursor) {
                // If next is None, we know the range is empty, return directly
                None => return (Some(self.tree.end_cursor()), entity_index),
                Some(x) => {
                    assert_eq!(right.offset, elem.rle_len());
                    right = x;
                    let mut elem = self.tree.get_elem(right.leaf).unwrap();
                    if matches!(elem, RichtextStateChunk::Text { .. }) {
                        return (Some(right), entity_index);
                    }

                    let left = x;
                    while matches!(elem, RichtextStateChunk::Style { .. }) {
                        match self.tree.next_elem(right) {
                            Some(r) => {
                                right = r;
                                elem = self.tree.get_elem(right.leaf).unwrap();
                            }
                            None => {
                                // this is last element
                                right.offset = 1;
                                break;
                            }
                        }
                    }

                    (left, right, entity_index)
                }
            }
        };

        let mut iter = left;

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

            visited.push((style, anchor_type, iter, entity_index));
            if anchor_type == AnchorType::Start {
                // case 1. should be before this anchor
                break;
            }

            if style.info.prefer_insert_before(anchor_type) {
                // case 2.
                break;
            }

            iter = match self.tree.next_elem(iter) {
                Some(x) => x,
                None => self.tree.end_cursor(),
            };
            entity_index += 1;
        }

        while let Some((style, anchor_type, top_elem, top_entity_index)) = visited.pop() {
            if !style.info.prefer_insert_before(anchor_type) {
                // case 3.
                break;
            }

            iter = top_elem;
            entity_index = top_entity_index;
        }

        (Some(iter), entity_index)
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

    pub(crate) fn get_text_entity_ranges(
        &self,
        pos: usize,
        len: usize,
        pos_type: PosType,
    ) -> Vec<Range<usize>> {
        if self.tree.is_empty() {
            return Vec::new();
        }

        if len == 0 {
            return Vec::new();
        }

        let mut ans: Vec<Range<usize>> = Vec::new();
        let (start, end) = match pos_type {
            PosType::Bytes => todo!(),
            PosType::Unicode => (
                self.tree.query::<UnicodeQuery>(&pos).unwrap().cursor,
                self.tree
                    .query::<UnicodeQuery>(&(pos + len))
                    .unwrap()
                    .cursor,
            ),
            PosType::Utf16 => (
                self.tree.query::<Utf16Query>(&pos).unwrap().cursor,
                self.tree.query::<Utf16Query>(&(pos + len)).unwrap().cursor,
            ),
            PosType::Entity => (
                self.tree.query::<EntityQuery>(&pos).unwrap().cursor,
                self.tree.query::<EntityQuery>(&(pos + len)).unwrap().cursor,
            ),
            PosType::Event => (
                self.tree.query::<EventIndexQuery>(&pos).unwrap().cursor,
                self.tree
                    .query::<EventIndexQuery>(&(pos + len))
                    .unwrap()
                    .cursor,
            ),
        };

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
        mut f: impl FnMut(RichtextStateChunk),
    ) -> (usize, usize) {
        assert!(
            pos + len <= self.len_entity(),
            "pos: {}, len: {}, self.len(): {}",
            pos,
            len,
            &self.to_string()
        );
        // PERF: may use cache to speed up
        self.cursor_cache.invalidate();
        // FIXME: need to check whether style is removed when its anchors are removed
        self.style_ranges.delete(pos..pos + len);
        let range = pos..pos + len;
        let (start, start_f) = self
            .tree
            .query_with_finder_return::<EntityIndexQueryWithEventIndex>(&range.start);
        let start_cursor = start.unwrap().cursor();
        let elem = self.tree.get_elem(start_cursor.leaf).unwrap();
        if elem.rle_len() >= start_cursor.offset + len {
            // drop in place
            let mut event_len = 0;
            self.tree.update_leaf(start_cursor.leaf, |elem| match elem {
                RichtextStateChunk::Text(text) => {
                    let (next, event_len_) = text.delete_by_entity_index(start_cursor.offset, len);
                    event_len = event_len_;
                    (true, next.map(RichtextStateChunk::Text), None)
                }
                RichtextStateChunk::Style { .. } => {
                    *elem = RichtextStateChunk::Text(TextChunk::new_empty());
                    (true, None, None)
                }
            });
            return (start_f.event_index, start_f.event_index + event_len);
        }
        let (end, end_f) = self
            .tree
            .query_with_finder_return::<EntityIndexQueryWithEventIndex>(&range.end);
        for iter in generic_btree::iter::Drain::new(&mut self.tree, start, end) {
            f(iter)
        }
        (start_f.event_index, end_f.event_index)
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

    pub fn iter(&self) -> impl Iterator<Item = RichtextSpan> + '_ {
        let mut entity_index = 0;
        let mut style_range_iter = self.style_ranges.iter();
        let mut cur_style_range = style_range_iter.next();
        let mut cur_styles: Option<StyleMeta> =
            cur_style_range.as_ref().map(|x| x.1.clone().into());

        self.tree.iter().filter_map(move |x| match x {
            RichtextStateChunk::Text(s) => {
                let mut styles = Default::default();
                while let Some((inner_cur_range, _)) = cur_style_range.as_ref() {
                    if entity_index < inner_cur_range.start {
                        break;
                    }

                    if entity_index < inner_cur_range.end {
                        styles = cur_styles.as_ref().unwrap().clone();
                        break;
                    } else {
                        cur_style_range = style_range_iter.next();
                        cur_styles = cur_style_range.as_ref().map(|x| x.1.clone().into());
                    }
                }

                entity_index += s.rle_len();
                Some(RichtextSpan {
                    text: s.bytes().clone().into(),
                    attributes: styles,
                })
            }
            RichtextStateChunk::Style { .. } => {
                entity_index += 1;
                None
            }
        })
    }

    #[inline]
    pub fn iter_chunk(&self) -> impl Iterator<Item = &RichtextStateChunk> {
        self.tree.iter()
    }

    pub fn get_richtext_value(&self) -> LoroValue {
        let mut ans: Vec<LoroValue> = Vec::new();
        let mut last_attributes: Option<LoroValue> = None;
        for span in self.iter() {
            let attributes: LoroValue = span.attributes.to_value();
            if let Some(last) = last_attributes.as_ref() {
                if &attributes == last {
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

            if !span.attributes.is_empty() {
                value.insert("attributes".into(), attributes.clone());
            }

            ans.push(LoroValue::Map(Arc::new(value)));
            last_attributes = Some(attributes);
        }

        LoroValue::List(Arc::new(ans))
    }

    #[allow(unused)]
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

    pub fn diagnose(&self) {
        CursorCache::diagnose();
        println!(
            "rope_nodes: {}, style_nodes: {}, text_len: {}",
            self.tree.node_len(),
            self.style_ranges.tree.node_len(),
            self.tree.root_cache().bytes
        );
    }
}

#[cfg(test)]
mod test {
    use append_only_bytes::AppendOnlyBytes;
    use serde_json::json;

    use crate::{container::richtext::TextStyleInfoFlag, ToJson};

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
            {
                let state = &mut self.state;
                let text = self.bytes.slice(start..);
                let entity_index = state.get_entity_index_for_text_insert(pos, PosType::Unicode);
                state.insert_at_entity_index(entity_index, text);
            };
        }

        fn delete(&mut self, pos: usize, len: usize) {
            let ranges = self
                .state
                .get_text_entity_ranges(pos, len, PosType::Unicode);
            for range in ranges {
                self.state
                    .drain_by_entity_index(range.start, range.end - range.start, |_| {});
            }
        }

        fn mark(&mut self, range: Range<usize>, style: Arc<StyleOp>) {
            let start = self
                .state
                .get_entity_index_for_text_insert(range.start, PosType::Unicode);
            let end = self
                .state
                .get_entity_index_for_text_insert(range.end, PosType::Unicode);
            self.state.mark_with_entity_index(start..end, style);
        }
    }

    fn bold(n: isize) -> Arc<StyleOp> {
        Arc::new(StyleOp::new_for_test(
            n,
            "bold",
            true.into(),
            TextStyleInfoFlag::BOLD,
        ))
    }

    fn comment(n: isize) -> Arc<StyleOp> {
        Arc::new(StyleOp::new_for_test(
            n,
            "comment",
            "comment".into(),
            TextStyleInfoFlag::COMMENT,
        ))
    }

    fn unbold(n: isize) -> Arc<StyleOp> {
        Arc::new(StyleOp::new_for_test(
            n,
            "bold",
            LoroValue::Null,
            TextStyleInfoFlag::BOLD.to_delete(),
        ))
    }

    fn link(n: isize) -> Arc<StyleOp> {
        Arc::new(StyleOp::new_for_test(
            n,
            "link",
            true.into(),
            TextStyleInfoFlag::LINK,
        ))
    }

    #[test]
    fn test() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello World!");
        wrapper.mark(0..5, bold(0));
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": "Hello",
                    "attributes": {
                        "bold": true
                    }
                },
                {
                    "insert": " World!"
                }
            ])
        );
        wrapper.mark(2..7, link(1));
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": "He",
                    "attributes": {
                        "bold": true
                    }
                },
                {
                    "insert": "llo",
                    "attributes": {
                        "bold": true,
                        "link": true
                    }
                },
                {
                    "insert": " W",
                    "attributes": {
                        "link": true
                    }
                },
                {
                    "insert": "orld!"
                }

            ])
        );
    }

    #[test]
    fn delete_text() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello World!");
        wrapper.delete(0, 5);
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": " World!"
                }
            ])
        );

        wrapper.delete(1, 1);

        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": " orld!"
                }
            ])
        );

        wrapper.delete(5, 1);
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": " orld"
                }
            ])
        );

        wrapper.delete(0, 5);
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([])
        );
    }

    #[test]
    #[ignore]
    fn insert_cache_hit() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "H");
        wrapper.insert(1, "H");
        dbg!(&wrapper);
        wrapper.insert(2, "e");
        dbg!(&wrapper);
        wrapper.insert(3, "k");
        wrapper.state.diagnose();
    }

    #[test]
    fn bold_should_expand() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello World!");
        wrapper.mark(0..5, bold(0));
        wrapper.insert(5, " Test");
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": "Hello Test",
                    "attributes": {
                        "bold": true
                    }
                },
                {
                    "insert": " World!"
                }
            ])
        );
    }

    #[test]
    fn link_should_not_expand() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello World!");
        wrapper.mark(0..5, link(0));
        wrapper.insert(5, " Test");
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": "Hello",
                    "attributes": {
                        "link": true
                    }
                },
                {
                    "insert": " Test World!"
                },
            ])
        );
    }

    #[test]
    fn continuous_text_insert_should_be_merged() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello");
        wrapper.insert(5, " World!");
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": "Hello World!"
                },
            ])
        );
    }

    #[test]
    fn continuous_text_insert_should_be_merged_and_have_bold() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello");
        wrapper.mark(0..5, bold(0));
        wrapper.insert(5, " World!");
        dbg!(&wrapper.state);
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": "Hello World!",
                    "attributes": {
                        "bold": true
                    }
                },
            ])
        );
    }

    #[test]
    fn continuous_text_insert_should_not_be_merged_when_prev_is_link() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello");
        wrapper.mark(0..5, link(0));
        wrapper.insert(5, " World!");
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": "Hello",
                    "attributes": {
                        "link": true
                    }
                },
                {

                    "insert": " World!",
                }
            ])
        );
    }

    #[test]
    fn delete_bold() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello World!");
        wrapper.mark(0..12, bold(0));
        wrapper.mark(5..12, unbold(1));
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": "Hello",
                    "attributes": {
                        "bold": true
                    }
                },
                {
                    "insert": " World!",
                    "attributes": {
                        "bold": null
                    }
                }
            ])
        );
        wrapper.insert(5, "A");
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": "HelloA",
                    "attributes": {
                        "bold": true
                    }
                },
                {
                    "insert": " World!",
                    "attributes": {
                        "bold": null
                    }
                }
            ])
        );

        wrapper.insert(0, "A");
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": "A",
                },
                {
                    "insert": "HelloA",
                    "attributes": {
                        "bold": true
                    }
                },
                {
                    "insert": " World!",
                    "attributes": {
                        "bold": null
                    }
                }
            ])
        );
    }

    #[test]
    fn bold_and_link_at_the_same_place() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello");
        wrapper.mark(0..5, link(0));
        wrapper.mark(0..5, bold(1));
        wrapper.insert(5, "A");
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": "Hello",
                    "attributes": {
                        "bold": true,
                        "link": true
                    }
                },
                {
                    "insert": "A",
                    "attributes": {
                        "bold": true,
                    }
                },
            ])
        );
    }

    #[test]
    fn test_comments() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello World!");
        wrapper.mark(0..5, comment(0));
        wrapper.mark(1..6, comment(1));
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": "H",
                    "attributes": {
                        "id:0@0": {
                            "key": "comment",
                            "data": "comment"
                        },
                    },
                },
                {
                    "insert": "ello",
                    "attributes": {
                        "id:0@0": {
                            "key": "comment",
                            "data": "comment"
                        },
                        "id:1@1": {
                            "key": "comment",
                            "data": "comment"
                        }
                    },
                },

                {
                    "insert": " ",
                    "attributes": {
                        "id:1@1": {
                            "key": "comment",
                            "data": "comment"
                        }
                    },
                },
                {
                    "insert": "World!",
                }
            ])
        );
    }

    #[test]
    fn remove_style_anchors_should_also_delete_style() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello World!");
        wrapper.mark(0..5, bold(0));
        let mut count = 0;
        wrapper.state.drain_by_entity_index(0, 7, |span| {
            if matches!(span, RichtextStateChunk::Style { .. }) {
                count += 1;
            }
        });

        assert_eq!(count, 2);
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([{
                "insert": " World!"
            }])
        );
    }
}
