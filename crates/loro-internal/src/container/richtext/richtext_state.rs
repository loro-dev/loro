use append_only_bytes::BytesSlice;
use generic_btree::{
    rle::{CanRemove, HasLength, Mergeable, Sliceable, TryInsert},
    BTree, BTreeTrait, Cursor, LeafIndex,
};
use loro_common::{Counter, IdFull, IdSpan, LoroError, LoroResult, LoroValue, ID};
use query::{ByteQuery, ByteQueryT};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{ser::SerializeStruct, Serialize};
use std::{
    fmt::{Display, Formatter},
    ops::{Bound, RangeBounds},
};
use std::{
    ops::{Add, AddAssign, Range, Sub},
    str::Utf8Error,
    sync::Arc,
};
use tracing::instrument;

use crate::{
    container::richtext::style_range_map::EMPTY_STYLES,
    delta::{DeltaValue, StyleMeta},
    utils::query_by_len::{EntityIndexQueryWithEventIndex, IndexQueryWithEntityIndex, QueryByLen},
};

use self::query::{
    EntityQuery, EntityQueryT, EventIndexQuery, EventIndexQueryT, UnicodeQuery, UnicodeQueryT,
    Utf16Query, Utf16QueryT,
};

use super::{
    style_range_map::{IterAnchorItem, StyleRangeMap, Styles},
    AnchorType, RichtextSpan, StyleKey, StyleOp,
};

pub(crate) use crate::cursor::PosType;

#[derive(Clone, Debug, Default)]
pub(crate) struct RichtextState {
    tree: BTree<RichtextTreeTrait>,
    style_ranges: Option<Box<StyleRangeMap>>,
    cached_cursor: Option<CachedCursor>,
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

use cache::CachedCursor;

mod cache {
    use super::*;

    #[derive(Clone, Debug)]
    pub(super) struct CachedCursor {
        leaf: LeafIndex,
        index: FxHashMap<PosType, usize>,
    }

    impl RichtextState {
        pub(super) fn clear_cache(&mut self) {
            self.cached_cursor = None;
        }

        pub(super) fn record_cache(
            &mut self,
            leaf: LeafIndex,
            pos: usize,
            pos_type: PosType,
            entity_offset: usize,
            entity_index: Option<usize>,
        ) {
            let offset = if entity_offset != 0 {
                let elem = self.tree.get_elem(leaf).unwrap();
                entity_offset_to_pos_type_offset(pos_type, elem, entity_offset)
            } else {
                0
            };

            let pos = pos - offset;
            match &mut self.cached_cursor {
                Some(c) => {
                    if c.leaf == leaf {
                        c.index.insert(pos_type, pos);
                    } else {
                        c.leaf = leaf;
                        c.index.clear();
                        c.index.insert(pos_type, pos);
                    }
                }
                None => {
                    self.cached_cursor = Some(CachedCursor {
                        leaf,
                        index: FxHashMap::default(),
                    });
                    self.cached_cursor
                        .as_mut()
                        .unwrap()
                        .index
                        .insert(pos_type, pos);
                }
            }
            if let Some(entity_index) = entity_index {
                self.cached_cursor
                    .as_mut()
                    .unwrap()
                    .index
                    .insert(PosType::Entity, entity_index - entity_offset);
            }
        }

        pub(super) fn try_get_cache_or_clean(
            &mut self,
            index: usize,
            pos_type: PosType,
        ) -> Option<Cursor> {
            let mut cursor = self.cached_cursor.take();
            let ans = 'block: {
                match &mut cursor {
                    Some(c) => {
                        let cache_index = c.index.get(&pos_type);
                        if let Some(cache_index) = cache_index {
                            if index < *cache_index {
                                return None;
                            }
                        }

                        let cached_index = match c.index.entry(pos_type) {
                            std::collections::hash_map::Entry::Vacant(vacant_entry) => {
                                let index = self.get_index_from_cursor(
                                    Cursor {
                                        leaf: c.leaf,
                                        offset: 0,
                                    },
                                    pos_type,
                                )?;
                                vacant_entry.insert(index);
                                index
                            }
                            std::collections::hash_map::Entry::Occupied(occupied_entry) => {
                                *occupied_entry.get()
                            }
                        };

                        if index < cached_index {
                            return None;
                        }

                        if cached_index == index {
                            break 'block Some(Cursor {
                                leaf: c.leaf,
                                offset: 0,
                            });
                        }

                        let elem = self.tree.get_elem(c.leaf)?;
                        let elem_len = elem.len_with(pos_type);
                        if cached_index + elem_len == index {
                            let offset = elem.len_with(PosType::Entity);
                            break 'block Some(Cursor {
                                leaf: c.leaf,
                                offset,
                            });
                        }

                        if index > cached_index + elem_len {
                            return None;
                        }

                        let offset =
                            pos_type_offset_to_entity_offset(pos_type, elem, index - cached_index)?;
                        Some(Cursor {
                            leaf: c.leaf,
                            offset,
                        })
                    }
                    None => None,
                }
            };

            self.cached_cursor = cursor;
            #[cfg(debug_assertions)]
            {
                if let Some(c) = ans.as_ref() {
                    let actual = self.get_index_from_cursor(*c, pos_type);
                    assert_eq!(actual.unwrap(), index);
                }
            }
            ans
        }

        pub(super) fn get_cache_entity_index(&mut self) -> Option<usize> {
            let mut cursor = self.cached_cursor.take()?;
            let ans = {
                let leaf = cursor.leaf;
                match cursor.index.entry(PosType::Entity) {
                    std::collections::hash_map::Entry::Vacant(vacant_entry) => {
                        let index = self
                            .get_index_from_cursor(Cursor { leaf, offset: 0 }, PosType::Entity)?;
                        vacant_entry.insert(index);
                        index
                    }
                    std::collections::hash_map::Entry::Occupied(occupied_entry) => {
                        *occupied_entry.get()
                    }
                }
            };

            self.cached_cursor = Some(cursor);
            Some(ans)
        }

        pub(crate) fn check_cache(&self) {
            #[cfg(debug_assertions)]
            {
                if let Some(c) = &self.cached_cursor {
                    for (pos_type, index) in &c.index {
                        let actual = self.get_index_from_cursor(
                            Cursor {
                                leaf: c.leaf,
                                offset: 0,
                            },
                            *pos_type,
                        );

                        assert_eq!(actual.unwrap(), *index);
                    }
                }
            }
        }
    }
}

pub(crate) use text_chunk::TextChunk;
mod text_chunk {
    use std::ops::Range;

    use append_only_bytes::BytesSlice;
    use loro_common::{IdFull, ID};

    #[derive(Clone, PartialEq)]
    pub(crate) struct TextChunk {
        bytes: BytesSlice,
        unicode_len: i32,
        utf16_len: i32,
        id: IdFull,
    }

    impl std::fmt::Debug for TextChunk {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("TextChunk")
                .field("text", &self.as_str())
                .field("unicode_len", &self.unicode_len)
                .field("utf16_len", &self.utf16_len)
                .field("id", &self.id)
                .finish()
        }
    }

    impl TextChunk {
        pub fn new(bytes: BytesSlice, id: IdFull) -> Self {
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
                id,
            }
        }

        #[inline]
        pub fn id(&self) -> ID {
            self.id.id()
        }

        #[inline]
        #[allow(unused)]
        pub fn id_full(&self) -> IdFull {
            self.id
        }

        #[inline]
        pub fn bytes(&self) -> &BytesSlice {
            &self.bytes
        }

        #[inline]
        pub fn as_str(&self) -> &str {
            // SAFETY: We know that the text is valid UTF-8
            unsafe { std::str::from_utf8_unchecked(&self.bytes) }
        }

        #[inline]
        pub fn len(&self) -> i32 {
            self.unicode_len
        }

        #[inline]
        pub fn utf8_len(&self) -> i32 {
            self.bytes.len() as i32
        }

        #[inline]
        pub fn unicode_len(&self) -> i32 {
            self.unicode_len
        }

        #[inline]
        pub fn utf16_len(&self) -> i32 {
            self.utf16_len
        }

        #[inline]
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
                // This is a dummy value.
                // It's fine because the length is 0. We never actually use this value.
                id: IdFull::NONE_ID,
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
                    self.id = self.id.inc(end_unicode_index as i32);
                    None
                }
                (false, true) => {
                    self.bytes.slice_(..start_byte);
                    None
                }
                (false, false) => {
                    let next = self.bytes.slice_clone(end_byte..);
                    let next = Self::new(next, self.id.inc(end_unicode_index as i32));
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
            if cfg!(any(debug_assertions, test)) {
                assert_eq!(self.unicode_len, self.as_str().chars().count() as i32);
                assert_eq!(
                    self.utf16_len,
                    self.as_str().chars().map(|c| c.len_utf16()).sum::<usize>() as i32
                );
            }
        }

        pub(crate) fn entity_range_to_event_range(&self, range: Range<usize>) -> Range<usize> {
            if cfg!(feature = "wasm") {
                assert!(range.start <= range.end);
                if range.start == 0 && range.end == self.unicode_len as usize {
                    return 0..self.utf16_len as usize;
                }

                let mut start = 0;
                let mut end = 0;
                let mut utf16_index = 0;
                for (unicode_index, c) in self.as_str().chars().enumerate() {
                    if unicode_index == range.start {
                        start = utf16_index;
                    }
                    if unicode_index == range.end {
                        end = utf16_index;
                        break;
                    }

                    utf16_index += c.len_utf16();
                }

                if end == 0 {
                    end = utf16_index;
                }

                start..end
            } else {
                range
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
            let mut last_unicode_index = 0;
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

                last_unicode_index = unicode_index;
            }

            assert!(started);
            if end == 0 {
                assert_eq!(last_unicode_index + 1, range.end);
                end = self.bytes.len();
            }

            let ans = Self {
                unicode_len: range.len() as i32,
                bytes: self.bytes.slice_clone(start..end),
                utf16_len: utf16_len as i32,
                id: self.id.inc(range.start as i32),
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
                id: self.id.inc(pos as i32),
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
            self.bytes.can_merge(&rhs.bytes) && self.id.inc(self.unicode_len) == rhs.id
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
            self.id = left.id;
            self.check();
        }
    }
}

// TODO: change visibility back to crate after #116 is done
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RichtextStateChunk {
    Text(TextChunk),
    Style {
        style: Arc<StyleOp>,
        anchor_type: AnchorType,
    },
}

impl Default for RichtextStateChunk {
    fn default() -> Self {
        Self::new_empty()
    }
}

impl RichtextStateChunk {
    pub fn new_text(s: BytesSlice, id: IdFull) -> Self {
        Self::Text(TextChunk::new(s, id))
    }

    pub fn new_empty() -> Self {
        Self::Text(TextChunk::new_empty())
    }

    pub fn new_style(style: Arc<StyleOp>, anchor_type: AnchorType) -> Self {
        Self::Style { style, anchor_type }
    }

    pub(crate) fn get_id_span(&self) -> IdSpan {
        match self {
            RichtextStateChunk::Text(t) => {
                let id = t.id();
                IdSpan::new(id.peer, id.counter, id.counter + t.unicode_len() as Counter)
            }
            RichtextStateChunk::Style { style, anchor_type } => match anchor_type {
                AnchorType::Start => style.id().into(),
                AnchorType::End => {
                    let id = style.id();
                    id.to_span(1)
                }
            },
        }
    }

    pub(crate) fn counter(&self) -> Counter {
        match self {
            RichtextStateChunk::Text(t) => t.id().counter,
            RichtextStateChunk::Style { style, anchor_type } => match anchor_type {
                AnchorType::Start => style.id().counter,
                AnchorType::End => {
                    let id = style.id();
                    id.counter + 1
                }
            },
        }
    }

    pub fn entity_range_to_event_range(&self, range: Range<usize>) -> Range<usize> {
        match self {
            RichtextStateChunk::Text(t) => t.entity_range_to_event_range(range),
            RichtextStateChunk::Style { .. } => {
                assert_eq!(range.start, 0);
                assert_eq!(range.end, 1);
                0..1
            }
        }
    }

    pub fn len_with(&self, pos_type: PosType) -> usize {
        match self {
            RichtextStateChunk::Text(t) => match pos_type {
                PosType::Bytes => t.utf8_len() as usize,
                PosType::Utf16 => t.utf16_len() as usize,
                PosType::Event => t.unicode_len() as usize,
                PosType::Entity => t.unicode_len() as usize,
                PosType::Unicode => t.unicode_len() as usize,
            },
            RichtextStateChunk::Style { .. } => {
                if let PosType::Entity = pos_type {
                    1
                } else {
                    0
                }
            }
        }
    }
}

impl loro_delta::delta_trait::DeltaValue for RichtextStateChunk {}

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
    pub fn try_new(s: BytesSlice, id: IdFull) -> Result<Self, Utf8Error> {
        std::str::from_utf8(&s)?;
        Ok(RichtextStateChunk::Text(TextChunk::new(s, id)))
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

impl TryInsert for RichtextStateChunk {
    fn try_insert(&mut self, _pos: usize, elem: Self) -> Result<(), Self>
    where
        Self: Sized,
    {
        Err(elem)
    }
}

impl CanRemove for RichtextStateChunk {
    fn can_remove(&self) -> bool {
        self.rle_len() == 0
    }
}

//TODO: start/end can be scanned in one loop, but now it takes twice the time
fn unicode_slice(s: &str, start_index: usize, end_index: usize) -> Result<&str, ()> {
    let (Some(start), Some(end)) = (
        unicode_to_utf8_index(s, start_index),
        unicode_to_utf8_index(s, end_index),
    ) else {
        return Err(());
    };
    Ok(&s[start..end])
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
        current_utf16_index += c.len_utf16();
        if current_utf16_index == utf16_index {
            return Ok(i + 1);
        }
        if current_utf16_index > utf16_index {
            loro_common::info!("WARNING: UTF16 MISMATCHED!");
            return Err(i);
        }
        current_unicode_index = i + 1;
    }

    loro_common::info!("WARNING: UTF16 MISMATCHED!");
    Err(current_unicode_index)
}

pub(crate) fn utf8_to_unicode_index(s: &str, utf8_index: usize) -> Result<usize, usize> {
    if utf8_index == 0 {
        return Ok(0);
    }

    let mut current_utf8_index = 0;
    let mut current_unicode_index = 0;
    for (i, c) in s.chars().enumerate() {
        let char_start = current_utf8_index;
        current_utf8_index += c.len_utf8();

        if utf8_index == char_start {
            return Ok(i);
        }

        if utf8_index < current_utf8_index {
            loro_common::info!("WARNING: UTF-8 index is in the middle of a codepoint!");
            return Err(i);
        }
        current_unicode_index = i + 1;
    }

    if current_utf8_index == utf8_index {
        Ok(current_unicode_index)
    } else {
        Err(current_unicode_index)
    }
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Default)]
pub(crate) struct PosCache {
    pub(super) unicode_len: i32,
    pub(super) bytes: i32,
    pub(super) utf16_len: i32,
    pub(crate) entity_len: i32,
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

impl CanRemove for PosCache {
    fn can_remove(&self) -> bool {
        self.bytes == 0
    }
}

pub(crate) struct RichtextTreeTrait;

#[derive(Debug)]
pub(crate) struct EntityRangeInfo {
    pub id_start: ID,
    pub entity_start: usize,
    pub entity_end: usize,
    pub event_len: usize,
}

impl EntityRangeInfo {
    pub fn entity_len(&self) -> usize {
        self.entity_end - self.entity_start
    }
}

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
    use crate::utils::query_by_len::{IndexQuery, QueryByLen};

    use super::*;

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
                    // TODO: if we remove the use of query(pos-1), we won't need this fallback behavior
                    // WARNING: Unable to report error!!!
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

    pub(super) struct ByteQueryT;
    pub(super) type ByteQuery = IndexQuery<ByteQueryT, RichtextTreeTrait>;
    impl QueryByLen<RichtextTreeTrait> for ByteQueryT {
        fn get_cache_len(cache: &<RichtextTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.bytes as usize
        }
        fn get_elem_len(elem: &<RichtextTreeTrait as BTreeTrait>::Elem) -> usize {
            match elem {
                RichtextStateChunk::Text(s) => s.utf8_len() as usize,
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
                    // TODO: if we remove the use of query(pos-1), we won't need this fallback behavior
                    // WARNING: Unable to report error!!!
                    let offset = utf8_to_unicode_index(s.as_str(), left).unwrap_or_else(|e| e);
                    (offset, true)
                }
                RichtextStateChunk::Style { .. } => (1, false),
            }
        }

        fn get_cache_entity_len(cache: &<RichtextTreeTrait as BTreeTrait>::Cache) -> usize {
            cache.entity_len as usize
        }
    }
}

impl RichtextState {
    pub(crate) fn from_chunks<I: Iterator<Item = impl Into<RichtextStateChunk>>>(i: I) -> Self {
        Self {
            tree: i.collect(),
            style_ranges: Default::default(),
            cached_cursor: None,
        }
    }

    pub(crate) fn get_entity_index_for_text_insert(
        &mut self,
        pos: usize,
        pos_type: PosType,
    ) -> Result<(usize, Option<Cursor>), LoroError> {
        self.check_cache();
        let result = {
            if self.tree.is_empty() {
                return Ok((0, None));
            }

            if let Some(c) = self.try_get_cache_or_clean(pos, pos_type) {
                let entity_index = self.get_cache_entity_index().unwrap();
                Ok((entity_index + c.offset, Some(c)))
            } else {
                let (c, entity_index) = match pos_type {
                    PosType::Bytes => self.find_best_insert_pos::<ByteQueryT>(pos),
                    PosType::Unicode => self.find_best_insert_pos::<UnicodeQueryT>(pos),
                    PosType::Utf16 => self.find_best_insert_pos::<Utf16QueryT>(pos),
                    PosType::Entity => self.find_best_insert_pos::<EntityQueryT>(pos),
                    PosType::Event => self.find_best_insert_pos::<EventIndexQueryT>(pos),
                };

                if let Some(c) = c {
                    debug_assert_eq!(
                        self.get_index_from_cursor(c, PosType::Entity).unwrap(),
                        entity_index
                    );
                    self.record_cache(
                        c.leaf,
                        entity_index,
                        PosType::Entity,
                        c.offset,
                        Some(entity_index),
                    );
                }
                Ok((entity_index, c))
            }
        };
        self.check_cache();
        result
    }

    pub(crate) fn has_styles(&self) -> bool {
        self.style_ranges
            .as_ref()
            .map(|x| x.has_style())
            .unwrap_or(false)
    }

    pub(crate) fn range_has_style_key(
        &mut self,
        range: Range<usize>,
        key: &StyleKey,
    ) -> bool {
        self.check_cache();
        let result = match self.style_ranges.as_ref() {
            Some(s) => s.range_contains_key(range, key),
            None => false,
        };
        self.check_cache();
        result
    }

    /// Return the entity range and text styles at the given range.
    /// If in the target range the leaves are not in the same span, the returned styles would be None
    pub(crate) fn get_entity_range_and_text_styles_at_range(
        &mut self,
        range: Range<usize>,
        pos_type: PosType,
    ) -> (Range<usize>, Option<&Styles>) {
        self.check_cache();
        let result = {
            if self.tree.is_empty() {
                return (0..0, None);
            }

            let (start, _) = self
                .get_entity_index_for_text_insert(range.start, pos_type)
                .unwrap();
            let (end, _) = self
                .get_entity_index_for_text_insert(range.end, pos_type)
                .unwrap();
            if self.has_styles() {
                (
                    start..end,
                    self.style_ranges
                        .as_ref()
                        .unwrap()
                        .get_styles_of_range(start..end),
                )
            } else {
                (start..end, None)
            }
        };
        self.check_cache();
        result
    }

    /// Get the insert text styles at the given entity index if we insert text at that position
    ///
    // TODO: PERF we can avoid this calculation by getting it when inserting new text
    // but that requires a lot of changes
    pub(crate) fn get_styles_at_entity_index_for_insert(
        &mut self,
        entity_index: usize,
    ) -> StyleMeta {
        self.check_cache();
        let result = {
            if !self.has_styles() {
                return Default::default();
            }

            self.style_ranges
                .as_mut()
                .unwrap()
                .get_styles_for_insert(entity_index)
        };
        self.check_cache();
        result
    }

    /// This is used to accept changes from DiffCalculator
    pub(crate) fn insert_at_entity_index(
        &mut self,
        entity_index: usize,
        text: BytesSlice,
        id: IdFull,
    ) -> Cursor {
        self.check_cache();
        let result = {
            let elem = RichtextStateChunk::try_new(text, id).unwrap();
            self.style_ranges
                .as_mut()
                .map(|x| x.insert(entity_index, elem.rle_len()));
            let q = &entity_index;
            if let Some(c) = self.try_get_cache_or_clean(entity_index, PosType::Entity) {
                let p = self.tree.prefer_left(c).unwrap_or(c);
                self.tree.insert_by_path(p, elem).0
            } else {
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
            }
        };
        self.record_cache(
            result.leaf,
            entity_index,
            PosType::Entity,
            result.offset,
            None,
        );
        self.check_cache();
        result
    }

    /// This is used to accept changes from DiffCalculator.
    ///
    /// Return (event_index, styles)
    pub(crate) fn insert_elem_at_entity_index(
        &mut self,
        entity_index: usize,
        elem: RichtextStateChunk,
    ) -> (usize, &Styles) {
        self.clear_cache();
        let result = {
            debug_assert!(
                entity_index <= self.len_entity(),
                "entity_index={} len={} self={:#?}",
                entity_index,
                self.len_entity(),
                &self
            );

            let (c, f) = self
                .tree
                .query_with_finder_return::<EntityIndexQueryWithEventIndex>(&entity_index);
            let cursor = c.map(|x| x.cursor);
            let event_index = f.event_index;
            self.clear_cache();

            match cursor {
                Some(cursor) => {
                    let styles = self
                        .style_ranges
                        .as_mut()
                        .map(|x| x.insert(entity_index, elem.rle_len()))
                        .unwrap_or(&EMPTY_STYLES);
                    self.tree.insert_by_path(cursor, elem);
                    (event_index, styles)
                }
                None => {
                    let styles = self
                        .style_ranges
                        .as_mut()
                        .map(|x| x.insert(entity_index, elem.rle_len()))
                        .unwrap_or(&EMPTY_STYLES);
                    self.tree.push(elem);
                    (0, styles)
                }
            }
        };
        result
    }

    /// Convert cursor position to event index:
    ///
    /// - If feature="wasm", index is utf16 index,
    /// - If feature!="wasm", index is unicode index,
    pub(crate) fn cursor_to_event_index(&self, cursor: Cursor) -> usize {
        self.check_cache();
        let result = self.get_index_from_cursor(cursor, PosType::Event).unwrap();
        self.check_cache();
        result
    }

    pub(crate) fn cursor_to_unicode_index(&self, cursor: Cursor) -> usize {
        self.check_cache();
        let result = self
            .get_index_from_cursor(cursor, PosType::Unicode)
            .unwrap();
        self.check_cache();
        result
    }

    /// This method only updates `style_ranges`.
    /// When this method is called, the style start anchor and the style end anchor should already have been inserted.
    pub(crate) fn annotate_style_range(&mut self, range: Range<usize>, style: Arc<StyleOp>) {
        self.check_cache();
        self.clear_cache();
        self.ensure_style_ranges_mut().annotate(range, style, None);
        self.check_cache();
    }

    /// This method only updates `style_ranges`.
    /// When this method is called, the style start anchor and the style end anchor should already have been inserted.
    ///
    /// This method will return the event of this annotation in event length
    pub(crate) fn annotate_style_range_with_event(
        &mut self,
        range: Range<usize>,
        style: Arc<StyleOp>,
    ) -> impl Iterator<Item = (StyleMeta, usize)> + '_ {
        self.check_cache();
        self.clear_cache();
        let mut ranges_in_entity_index: Vec<(StyleMeta, Range<usize>)> = Vec::new();
        let mut start = range.start;
        let end = range.end;
        self.ensure_style_ranges_mut().annotate(
            range,
            style,
            Some(&mut |s, len| {
                let range = start..start + len;
                start += len;
                ranges_in_entity_index.push((s.into(), range));
            }),
        );

        assert_eq!(ranges_in_entity_index.last().unwrap().1.end, end);
        let mut converter = ContinuousIndexConverter::new(self);
        let result = ranges_in_entity_index
            .into_iter()
            .filter_map(move |(meta, range)| {
                let start = converter.convert_entity_index_to_event_index(range.start);
                let end = converter.convert_entity_index_to_event_index(range.end);
                if end == start {
                    return None;
                }

                Some((meta, end - start))
            });
        self.check_cache();
        result
    }

    /// init style ranges if not initialized
    fn ensure_style_ranges_mut(&mut self) -> &mut StyleRangeMap {
        self.clear_cache();
        let result = {
            if self.style_ranges.is_none() {
                self.style_ranges = Some(Box::default());
            }

            self.style_ranges.as_mut().unwrap()
        };
        result
    }

    pub(crate) fn get_char_by_event_index(&self, pos: usize) -> Result<char, ()> {
        self.check_cache();
        let result = {
            let cursor = self.tree.query::<EventIndexQuery>(&pos).unwrap().cursor;
            let Some(str) = &self.tree.get_elem(cursor.leaf) else {
                return Err(());
            };
            if cfg!(not(feature = "wasm")) {
                let mut char_iter = str.as_str().unwrap().chars();
                match &mut char_iter.nth(cursor.offset) {
                    Some(c) => Ok(*c),
                    None => Err(()),
                }
            } else {
                let s = str.as_str().unwrap();
                let utf16offset = unicode_to_utf16_index(s, cursor.offset).unwrap();
                // Convert utf16 offset to actual character by finding the character at that position
                let char_iter = s.chars();
                let mut current_utf16_offset = 0;

                for c in char_iter {
                    if current_utf16_offset == utf16offset {
                        return Ok(c);
                    }
                    current_utf16_offset += c.len_utf16();
                    if current_utf16_offset > utf16offset {
                        return Err(());
                    }
                }
                Err(())
            }
        };
        self.check_cache();
        result
    }

    /// Find the best insert position based on the rich-text CRDT algorithm.
    ///
    /// The result is only different from `query` when there are style anchors around the insert pos.
    /// Returns the right neighbor of the insert pos and the entity index.
    ///
    /// 1. Insertions occur before style anchors that contain the beginning of new marks.
    /// 2. Insertions occur before style anchors that contain the end of bold-like marks
    /// 3. Insertions occur after style anchors that contain the end of link-like marks
    ///
    /// Rule 1 should be satisfied before rules 2 and 3 to avoid creating a new style out of nowhere
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
            let left = self.tree.start_cursor().unwrap();
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
            // The query prefers right when there are empty elements (style anchors)
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
                None => return (self.tree.end_cursor(), entity_index),
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
            if anchor_type == AnchorType::Start
                && (!style.value.is_null() || !style.value.is_false())
            {
                // case 1. should be before this anchor
                break;
            }

            if style.info.prefer_insert_before(anchor_type) {
                // case 2.
                break;
            }

            iter = match self.tree.next_elem(iter) {
                Some(x) => x,
                None => self.tree.end_cursor().unwrap(),
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
        self.get_index_from_cursor(right, PosType::Entity).unwrap()
    }

    pub fn get_index_from_cursor(
        &self,
        cursor: generic_btree::Cursor,
        pos_type: PosType,
    ) -> Option<usize> {
        let mut index = 0;
        self.tree
            .visit_previous_caches(cursor, |cache| match cache {
                generic_btree::PreviousCache::NodeCache(c) => {
                    index += c.get_len(pos_type) as usize;
                }
                generic_btree::PreviousCache::PrevSiblingElem(c) => {
                    index += c.len_with(pos_type);
                }
                generic_btree::PreviousCache::ThisElemAndOffset { elem, offset } => {
                    if offset != 0 {
                        index += entity_offset_to_pos_type_offset(pos_type, elem, offset)
                    }
                }
            });

        Some(index)
    }

    pub(crate) fn get_text_entity_ranges(
        &self,
        pos: usize,
        len: usize,
        pos_type: PosType,
    ) -> LoroResult<Vec<EntityRangeInfo>> {
        self.check_cache();
        let result = {
            if self.tree.is_empty() {
                return Ok(Vec::new());
            }

            if len == 0 {
                return Ok(Vec::new());
            }

            if pos + len > self.len(pos_type) {
                return Ok(Vec::new());
            }

            let mut ans: Vec<EntityRangeInfo> = Vec::new();
            let (start, end) = match pos_type {
                PosType::Bytes => (
                    self.tree.query::<ByteQuery>(&pos).unwrap().cursor,
                    self.tree.query::<ByteQuery>(&(pos + len)).unwrap().cursor,
                ),
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
                    RichtextStateChunk::Text(s) => {
                        let event_len = s.entity_range_to_event_range(start..end).len();
                        let id = s.id().inc(start as i32);
                        match ans.last_mut() {
                            Some(last)
                                if last.entity_end == entity_index
                                    && last.id_start.inc(last.event_len as i32) == id =>
                            {
                                last.entity_end += len;
                                last.event_len += event_len;
                            }
                            _ => {
                                ans.push(EntityRangeInfo {
                                    id_start: id,
                                    entity_start: entity_index,
                                    entity_end: entity_index + len,
                                    event_len,
                                });
                            }
                        }
                        entity_index += len;
                    }
                    RichtextStateChunk::Style { .. } => {
                        entity_index += 1;
                    }
                }
            }

            Ok(ans)
        };
        self.check_cache();
        result
    }

    pub(crate) fn get_text_slice_by_event_index(
        &self,
        pos: usize,
        len: usize,
    ) -> LoroResult<String> {
        self.check_cache();
        let result = {
            if self.tree.is_empty() {
                return Ok(String::new());
            }

            if len == 0 {
                return Ok(String::new());
            }

            if pos + len > self.len_event() {
                return Err(LoroError::OutOfBound {
                    pos: pos + len,
                    len: self.len_event(),
                    info: format!("Position: {}:{}", file!(), line!()).into_boxed_str(),
                });
            }

            let mut ans = String::new();
            let (start, end) = (
                self.tree.query::<EventIndexQuery>(&pos).unwrap().cursor,
                self.tree
                    .query::<EventIndexQuery>(&(pos + len))
                    .unwrap()
                    .cursor,
            );

            for span in self.tree.iter_range(start..end) {
                let start = span.start.unwrap_or(0);
                let end = span.end.unwrap_or(span.elem.rle_len());
                if end == 0 {
                    break;
                }

                if let RichtextStateChunk::Text(s) = span.elem {
                    match unicode_slice(s.as_str(), start, end) {
                        Ok(x) => ans.push_str(x),
                        Err(()) => {
                            return Err(LoroError::UTF16InUnicodeCodePoint { pos: pos + len })
                        }
                    }
                }
            }

            Ok(ans)
        };
        self.check_cache();
        result
    }

    pub(crate) fn slice_delta(
        &self,
        start_index: usize,
        end_index: usize,
        pos_type: PosType,
    ) -> LoroResult<Vec<(String, StyleMeta)>> {
        if end_index < start_index {
            return Err(LoroError::EndIndexLessThanStartIndex {
                start: start_index,
                end: end_index,
            });
        }

        if end_index == start_index {
            return Ok(Vec::new());
        }

        self.check_cache();
        let result = {
            // 1. Convert start/end pos to cursors
            let (start_query, end_query) = match pos_type {
                PosType::Event => (
                    self.tree.query::<EventIndexQuery>(&start_index),
                    self.tree.query::<EventIndexQuery>(&end_index),
                ),
                PosType::Bytes => (
                    self.tree.query::<ByteQuery>(&start_index),
                    self.tree.query::<ByteQuery>(&end_index),
                ),
                PosType::Unicode => (
                    self.tree.query::<UnicodeQuery>(&start_index),
                    self.tree.query::<UnicodeQuery>(&end_index),
                ),
                PosType::Utf16 => (
                    self.tree.query::<Utf16Query>(&start_index),
                    self.tree.query::<Utf16Query>(&end_index),
                ),
                PosType::Entity => (
                    self.tree.query::<EntityQuery>(&start_index),
                    self.tree.query::<EntityQuery>(&end_index),
                ),
            };

            let start_cursor = start_query
                .ok_or(LoroError::OutOfBound {
                    pos: start_index,
                    len: self.len(pos_type),
                    info: "".into(),
                })?
                .cursor;
            let end_cursor = end_query
                .ok_or(LoroError::OutOfBound {
                    pos: end_index,
                    len: self.len(pos_type),
                    info: "".into(),
                })?
                .cursor;

            // 2. Get start entity index
            let mut current_entity_index = self
                .get_index_from_cursor(start_cursor, PosType::Entity)
                .unwrap();
            // We need end entity index for style iterator
            let end_entity_index = self
                .get_index_from_cursor(end_cursor, PosType::Entity)
                .unwrap();

            // 3. Setup style iterator
            let mut style_range_iter: Box<dyn Iterator<Item = (Range<usize>, &Styles)>> =
                match &self.style_ranges {
                    Some(s) => {
                        let mut idx = current_entity_index;
                        Box::new(s.iter_range(current_entity_index..end_entity_index).map(
                            move |elem_slice| {
                                let len = elem_slice.end.unwrap_or(elem_slice.elem.len)
                                    - elem_slice.start.unwrap_or(0);
                                let range = idx..idx + len;
                                idx += len;
                                (range, &elem_slice.elem.styles)
                            },
                        ))
                    }
                    None => Box::new(Some((0..usize::MAX / 2, &*EMPTY_STYLES)).into_iter()),
                };

            let mut cur_style_range = style_range_iter.next();
            let mut cur_styles: Option<StyleMeta> =
                cur_style_range.as_ref().map(|x| x.1.clone().into());

            let mut ans: Vec<(String, StyleMeta)> = Vec::new();

            // 4. Iterate tree range
            for span in self.tree.iter_range(start_cursor..end_cursor) {
                match &span.elem {
                    RichtextStateChunk::Text(t) => {
                        let chunk_len =
                            span.end.unwrap_or(span.elem.rle_len()) - span.start.unwrap_or(0); // length in rle_len (unicode_len)
                        let mut processed_len = 0;

                        while processed_len < chunk_len {
                            while let Some((inner_cur_range, _)) = cur_style_range.as_ref() {
                                if current_entity_index >= inner_cur_range.end {
                                    cur_style_range = style_range_iter.next();
                                    cur_styles =
                                        cur_style_range.as_ref().map(|x| x.1.clone().into());
                                } else {
                                    break;
                                }
                            }

                            if cur_style_range.is_none() {
                                cur_styles = Some(StyleMeta::default());
                                cur_style_range =
                                    Some((current_entity_index..usize::MAX, &*EMPTY_STYLES));
                            }

                            let (inner_cur_range, _) = cur_style_range.as_ref().unwrap();

                            let remaining_text = chunk_len - processed_len;
                            let remaining_style = inner_cur_range.end - current_entity_index;
                            let take_len = remaining_text.min(remaining_style);

                            let slice_start = span.start.unwrap_or(0) + processed_len;
                            let slice_end = slice_start + take_len;

                            let text_content = unicode_slice(t.as_str(), slice_start, slice_end)
                                .map_err(|_| LoroError::OutOfBound {
                                    pos: slice_end,
                                    len: t.unicode_len() as usize,
                                    info: "Slice delta out of bound".into(),
                                })?;

                            let styles = cur_styles.as_ref().unwrap();
                            if let Some(last) = ans.last_mut() {
                                if &last.1 == styles {
                                    last.0.push_str(text_content);
                                    processed_len += take_len;
                                    current_entity_index += take_len;
                                    continue;
                                }
                            }

                            ans.push((text_content.to_string(), styles.clone()));

                            processed_len += take_len;
                            current_entity_index += take_len;
                        }
                    }
                    RichtextStateChunk::Style { .. } => {
                        current_entity_index += 1;
                    }
                }
            }

            Ok(ans)
        };
        self.check_cache();
        result
    }

    // PERF: can be splitted into two methods. One is without cursor_to_event_index
    // PERF: can be speed up a lot by detecting whether the range is in a single leaf first
    /// This is used to accept changes from DiffCalculator
    #[instrument(skip(self, f))]
    pub(crate) fn drain_by_entity_index(
        &mut self,
        pos: usize,
        len: usize,
        mut f: Option<&mut dyn FnMut(RichtextStateChunk)>,
    ) -> DrainInfo {
        let result = {
            assert!(
                pos + len <= self.len_entity(),
                "pos: {}, len: {}, self.len(): {}",
                pos,
                len,
                &self.len_entity(),
            );

            self.clear_cache();
            // PERF: may use cache to speed up
            let range = pos..pos + len;
            let (start, start_f) = self
                .tree
                .query_with_finder_return::<EntityIndexQueryWithEventIndex>(&range.start);
            let start_cursor = start.unwrap().cursor();
            let elem = self.tree.get_elem(start_cursor.leaf).unwrap();

            /// This struct remove the corresponding style ranges if the start style anchor is removed
            struct StyleRangeUpdater<'a> {
                style_ranges: Option<&'a mut StyleRangeMap>,
                current_index: usize,
                start: usize,
                end: usize,
            }

            impl<'a> StyleRangeUpdater<'a> {
                fn update(&mut self, elem: &RichtextStateChunk) {
                    match &elem {
                        RichtextStateChunk::Text(t) => {
                            self.current_index += t.unicode_len() as usize;
                        }
                        RichtextStateChunk::Style { style, anchor_type } => {
                            if matches!(anchor_type, AnchorType::End) {
                                self.end = self.end.max(self.current_index);
                                if let Some(s) = self.style_ranges.as_mut() {
                                    let start =
                                        s.remove_style_scanning_backward(style, self.current_index);
                                    self.start = self.start.min(start);
                                }
                            }

                            self.current_index += 1;
                        }
                    }
                }

                fn new(
                    style_ranges: Option<&'a mut Box<StyleRangeMap>>,
                    start_index: usize,
                ) -> Self {
                    Self {
                        style_ranges: style_ranges.map(|x| &mut **x),
                        current_index: start_index,
                        end: 0,
                        start: usize::MAX,
                    }
                }

                fn get_affected_range(&self, pos: usize) -> Option<Range<usize>> {
                    if self.start == usize::MAX {
                        None
                    } else {
                        let start = self.start.min(pos);
                        let end = self.end.min(pos);
                        if start == end {
                            None
                        } else {
                            Some(start..end)
                        }
                    }
                }
            }

            if elem.rle_len() >= start_cursor.offset + len {
                // drop in place
                let mut event_len = 0;
                let mut updater = StyleRangeUpdater::new(self.style_ranges.as_mut(), pos);
                self.tree.update_leaf(start_cursor.leaf, |elem| {
                    updater.update(&*elem);
                    match elem {
                        RichtextStateChunk::Text(text) => {
                            if let Some(f) = f {
                                let span =
                                    text.slice(start_cursor.offset..start_cursor.offset + len);
                                f(RichtextStateChunk::Text(span));
                            }
                            let (next, event_len_) =
                                text.delete_by_entity_index(start_cursor.offset, len);
                            event_len = event_len_;
                            (true, next.map(RichtextStateChunk::Text), None)
                        }
                        RichtextStateChunk::Style { .. } => {
                            if let Some(f) = f {
                                let v = std::mem::replace(
                                    elem,
                                    RichtextStateChunk::Text(TextChunk::new_empty()),
                                );
                                f(v);
                            } else {
                                *elem = RichtextStateChunk::Text(TextChunk::new_empty());
                            }
                            (true, None, None)
                        }
                    }
                });

                let affected_range = updater.get_affected_range(pos);
                if let Some(s) = self.style_ranges.as_mut() {
                    s.delete(pos..pos + len);
                }

                DrainInfo {
                    start_event_index: start_f.event_index,
                    end_event_index: (start_f.event_index + event_len),
                    affected_style_range: affected_range.map(|entity_range| {
                        (
                            entity_range.clone(),
                            self.entity_index_to_event_index(entity_range.start)
                                ..self.entity_index_to_event_index(entity_range.end),
                        )
                    }),
                }
            } else {
                let (end, end_f) = self
                    .tree
                    .query_with_finder_return::<EntityIndexQueryWithEventIndex>(&range.end);
                let mut updater = StyleRangeUpdater::new(self.style_ranges.as_mut(), pos);
                for iter in generic_btree::iter::Drain::new(&mut self.tree, start, end) {
                    updater.update(&iter);
                    if let Some(f) = f.as_mut() {
                        f(iter)
                    }
                }

                let affected_range = updater.get_affected_range(pos);
                if let Some(s) = self.style_ranges.as_mut() {
                    s.delete(pos..pos + len);
                }

                DrainInfo {
                    start_event_index: start_f.event_index,
                    end_event_index: end_f.event_index,
                    affected_style_range: affected_range.map(|entity_range| {
                        (
                            entity_range.clone(),
                            self.entity_index_to_event_index(entity_range.start)
                                ..self.entity_index_to_event_index(entity_range.end),
                        )
                    }),
                }
            }
        };
        result
    }

    pub fn entity_index_to_event_index(&self, index: usize) -> usize {
        if index == 0 {
            // the tree maybe empty
            return 0;
        }
        let cursor = self.tree.query::<EntityQuery>(&index).unwrap();
        self.cursor_to_event_index(cursor.cursor)
    }

    pub fn index_to_event_index(&self, index: usize, pos_type: PosType) -> usize {
        if self.tree.is_empty() {
            return 0;
        }

        let cursor = match pos_type {
            PosType::Entity => self.tree.query::<EntityQuery>(&index).unwrap(),
            PosType::Utf16 => self.tree.query::<Utf16Query>(&index).unwrap(),
            PosType::Bytes => self.tree.query::<ByteQuery>(&index).unwrap(),
            PosType::Event => return index,
            PosType::Unicode => self.tree.query::<UnicodeQuery>(&index).unwrap(),
        };

        self.cursor_to_event_index(cursor.cursor)
    }

    pub fn event_index_to_unicode_index(&self, index: usize) -> usize {
        if !cfg!(feature = "wasm") {
            return index;
        }

        let Some(cursor) = self.tree.query::<EventIndexQuery>(&index) else {
            return 0;
        };

        self.cursor_to_unicode_index(cursor.cursor)
    }

    #[allow(unused)]
    pub(crate) fn check(&self) {
        if !cfg!(any(debug_assertions, test)) {
            return;
        }

        self.tree.check();
        self.check_consistency_between_content_and_style_ranges();
        self.check_style_anchors_appear_in_pairs();
    }

    pub(crate) fn mark_with_entity_index(&mut self, range: Range<usize>, style: Arc<StyleOp>) {
        self.check_cache();
        self.clear_cache();
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
        self.ensure_style_ranges_mut()
            .annotate(range.start..range.end + 2, style, None);
        self.check_cache();
    }

    pub fn iter(&self) -> impl Iterator<Item = RichtextSpan> + '_ {
        let mut entity_index = 0;
        let mut style_range_iter: Box<dyn Iterator<Item = (Range<usize>, &Styles)>> =
            match &self.style_ranges {
                Some(s) => Box::new(s.iter()),
                None => Box::new(Some((0..usize::MAX / 2, &*EMPTY_STYLES)).into_iter()),
            };
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
        self.check_cache();
        let result = {
            let mut ans: Vec<LoroValue> = Vec::new();
            let mut last_attributes: Option<LoroValue> = None;
            for span in self.iter() {
                let attributes: LoroValue = span.attributes.to_value();
                if let Some(last) = last_attributes.as_ref() {
                    if &attributes == last {
                        let hash_map = ans.last_mut().unwrap().as_map_mut().unwrap();
                        let s = hash_map
                            .make_mut()
                            .get_mut("insert")
                            .unwrap()
                            .as_string_mut()
                            .unwrap();
                        s.make_mut().push_str(span.text.as_str());
                        continue;
                    }
                }

                let mut value = FxHashMap::default();
                value.insert(
                    "insert".into(),
                    LoroValue::String(span.text.as_str().into()),
                );

                if !attributes.as_map().unwrap().is_empty() {
                    value.insert("attributes".into(), attributes.clone());
                }

                ans.push(LoroValue::Map(value.into()));
                last_attributes = Some(attributes);
            }

            LoroValue::List(ans.into())
        };
        self.check_cache();
        result
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
    pub fn is_empty(&self) -> bool {
        self.tree.root_cache().entity_len == 0
    }

    #[inline(always)]
    pub fn len_entity(&self) -> usize {
        self.tree.root_cache().entity_len as usize
    }

    pub fn diagnose(&self) {
        println!(
            "rope_nodes: {}, style_nodes: {}, text_len: {}",
            self.tree.node_len(),
            self.style_ranges
                .as_ref()
                .map(|x| x.tree.node_len())
                .unwrap_or(0),
            self.tree.root_cache().bytes
        );
    }

    /// Check if the content and style ranges are consistent.
    ///
    /// Panic if inconsistent.
    pub(crate) fn check_consistency_between_content_and_style_ranges(&self) {
        if !cfg!(debug_assertions) {
            return;
        }

        let mut entity_index_to_style_anchor: FxHashMap<usize, &RichtextStateChunk> =
            FxHashMap::default();
        let mut index = 0;
        for c in self.iter_chunk() {
            if matches!(c, RichtextStateChunk::Style { .. }) {
                entity_index_to_style_anchor.insert(index, c);
            }

            index += c.length()
        }

        if let Some(s) = &self.style_ranges {
            for IterAnchorItem {
                index,
                op,
                anchor_type: iter_anchor_type,
            } in s.iter_anchors()
            {
                let c = entity_index_to_style_anchor
                    .remove(&index)
                    .unwrap_or_else(|| {
                        panic!(
                            "Inconsistency found {} {:?} {:?}",
                            index, iter_anchor_type, &op
                        );
                    });

                match c {
                    RichtextStateChunk::Text(_) => {
                        unreachable!()
                    }
                    RichtextStateChunk::Style { style, anchor_type } => {
                        assert_eq!(style, &op);
                        assert_eq!(&iter_anchor_type, anchor_type);
                    }
                }
            }
        }

        assert!(
            entity_index_to_style_anchor.is_empty(),
            "Inconsistency found. Some anchors are not reflected in style ranges {:#?}",
            &entity_index_to_style_anchor
        );
    }

    /// Allow StyleAnchors to appear in pairs, so that there won't be unmatched single StyleAnchors.
    pub(crate) fn check_style_anchors_appear_in_pairs(&self) {
        if !cfg!(any(debug_assertions, test)) {
            return;
        }

        let mut start_ops: FxHashSet<&Arc<StyleOp>> = Default::default();
        for item in self.iter_chunk() {
            match item {
                RichtextStateChunk::Text(_) => {}
                RichtextStateChunk::Style { style, anchor_type } => match anchor_type {
                    AnchorType::Start => {
                        start_ops.insert(style);
                    }
                    AnchorType::End => {
                        assert!(start_ops.remove(style), "End anchor without start anchor");
                    }
                },
            }
        }
        assert!(
            start_ops.is_empty(),
            "Only has start anchors {:#?}",
            &start_ops
        );
    }

    /// Iter style ranges in the given range in entity index
    pub(crate) fn iter_range(
        &self,
        range: impl RangeBounds<usize>,
    ) -> impl Iterator<Item = IterRangeItem<'_>> + '_ {
        self.check_cache();
        let result = {
            let start = match range.start_bound() {
                Bound::Included(x) => *x,
                Bound::Excluded(x) => x + 1,
                Bound::Unbounded => 0,
            };
            let end = match range.end_bound() {
                Bound::Included(x) => x + 1,
                Bound::Excluded(x) => *x,
                Bound::Unbounded => self.len_entity(),
            };
            assert!(end > start);
            assert!(end <= self.len_entity());

            let mut style_iter = self
                .style_ranges
                .as_ref()
                .map(|x| x.iter_range(range))
                .into_iter()
                .flatten();

            let start = self.tree.query::<EntityQuery>(&start).unwrap();
            let end = self.tree.query::<EntityQuery>(&end).unwrap();
            let mut content_iter = self.tree.iter_range(start.cursor..end.cursor);
            let mut style_left_len = usize::MAX;
            let mut cur_style = style_iter
                .next()
                .map(|x| {
                    style_left_len = x.elem.len - x.start.unwrap_or(0);
                    &x.elem.styles
                })
                .unwrap_or(&*EMPTY_STYLES);
            let mut chunk = content_iter.next();
            let mut offset = 0;
            let mut chunk_left_len = chunk
                .as_ref()
                .map(|x| {
                    let len = x.elem.rle_len();
                    offset = x.start.unwrap_or(0);
                    x.end.map(|v| v.min(len)).unwrap_or(len) - offset
                })
                .unwrap_or(0);
            std::iter::from_fn(move || {
                if chunk_left_len == 0 {
                    chunk = content_iter.next();
                    chunk_left_len = chunk
                        .as_ref()
                        .map(|x| {
                            let len = x.elem.rle_len();
                            x.end.map(|v| v.min(len)).unwrap_or(len)
                        })
                        .unwrap_or(0);
                    offset = 0;
                }

                let iter_chunk = chunk.as_ref()?;

                let styles = cur_style;
                let iter_len;
                let event_range;
                if chunk_left_len >= style_left_len {
                    iter_len = style_left_len;
                    event_range = iter_chunk
                        .elem
                        .entity_range_to_event_range(offset..offset + iter_len);
                    chunk_left_len -= style_left_len;
                    offset += style_left_len;
                    style_left_len = 0;
                } else {
                    iter_len = chunk_left_len;
                    event_range = iter_chunk
                        .elem
                        .entity_range_to_event_range(offset..offset + iter_len);
                    style_left_len -= chunk_left_len;
                    chunk_left_len = 0;
                }

                if style_left_len == 0 {
                    cur_style = style_iter
                        .next()
                        .map(|x| {
                            style_left_len = x.elem.len;
                            &x.elem.styles
                        })
                        .unwrap_or(&*EMPTY_STYLES);
                }

                Some(IterRangeItem {
                    chunk: iter_chunk.elem,
                    styles,
                    entity_len: iter_len,
                    event_len: event_range.len(),
                })
            })
        };
        self.check_cache();
        result
    }

    pub(crate) fn get_stable_position_at_event_index(
        &self,
        pos: usize,
        kind: PosType,
    ) -> Option<ID> {
        self.check_cache();
        let result = {
            let v = &self.get_text_entity_ranges(pos, 1, kind).unwrap();
            let a = v.first()?;
            Some(a.id_start)
        };
        self.check_cache();
        result
    }

    pub(crate) fn len_event(&self) -> usize {
        self.check_cache();
        let result = {
            if cfg!(feature = "wasm") {
                self.len_utf16()
            } else {
                self.len_unicode()
            }
        };
        self.check_cache();
        result
    }

    pub(crate) fn len(&self, pos_type: PosType) -> usize {
        self.check_cache();
        let result = {
            match pos_type {
                PosType::Unicode => self.len_unicode(),
                PosType::Utf16 => self.len_utf16(),
                PosType::Entity => self.len_entity(),
                PosType::Event => self.len_event(),
                PosType::Bytes => self.len_utf8(),
            }
        };
        self.check_cache();
        result
    }
}

fn entity_offset_to_pos_type_offset(
    pos_type: PosType,
    elem: &RichtextStateChunk,
    offset: usize,
) -> usize {
    match pos_type {
        PosType::Bytes => match elem {
            RichtextStateChunk::Text(t) => unicode_to_utf8_index(t.as_str(), offset).unwrap(),
            RichtextStateChunk::Style { .. } => 0,
        },
        PosType::Unicode => offset,
        PosType::Utf16 => match elem {
            RichtextStateChunk::Text(t) => unicode_to_utf16_index(t.as_str(), offset).unwrap(),
            RichtextStateChunk::Style { .. } => 0,
        },
        PosType::Entity => offset,
        PosType::Event => match elem {
            RichtextStateChunk::Text(t) => {
                if cfg!(feature = "wasm") {
                    unicode_to_utf16_index(t.as_str(), offset).unwrap()
                } else {
                    offset
                }
            }
            RichtextStateChunk::Style { .. } => 0,
        },
    }
}

fn pos_type_offset_to_entity_offset(
    pos_type: PosType,
    elem: &RichtextStateChunk,
    offset: usize,
) -> Option<usize> {
    match pos_type {
        PosType::Bytes => match elem {
            RichtextStateChunk::Text(t) => utf8_to_unicode_index(t.as_str(), offset).ok(),
            RichtextStateChunk::Style { .. } => {
                if offset > 0 {
                    None
                } else {
                    Some(0)
                }
            }
        },
        PosType::Unicode => Some(offset),
        PosType::Utf16 => match elem {
            RichtextStateChunk::Text(t) => utf16_to_unicode_index(t.as_str(), offset).ok(),
            RichtextStateChunk::Style { .. } => {
                if offset > 0 {
                    None
                } else {
                    Some(0)
                }
            }
        },
        PosType::Entity => {
            if offset > elem.rle_len() {
                None
            } else {
                Some(offset)
            }
        }
        PosType::Event => match elem {
            RichtextStateChunk::Text(t) => {
                if cfg!(feature = "wasm") {
                    utf16_to_unicode_index(t.as_str(), offset).ok()
                } else if offset < t.unicode_len() as usize {
                    Some(offset)
                } else {
                    None
                }
            }
            RichtextStateChunk::Style { .. } => {
                if offset > 0 {
                    None
                } else {
                    Some(0)
                }
            }
        },
    }
}

pub(crate) struct DrainInfo {
    pub start_event_index: usize,
    pub end_event_index: usize,
    // entity range, event range
    pub affected_style_range: Option<(Range<usize>, Range<usize>)>,
}

pub(crate) struct IterRangeItem<'a> {
    pub(crate) chunk: &'a RichtextStateChunk,
    pub(crate) styles: &'a Styles,
    pub(crate) entity_len: usize,
    pub(crate) event_len: usize,
}

use converter::ContinuousIndexConverter;
mod converter {
    use generic_btree::{rle::HasLength, Cursor};

    use super::{query::EntityQuery, RichtextState};

    /// Convert entity index into event index.
    /// It assumes the entity_index are in ascending order
    pub(super) struct ContinuousIndexConverter<'a> {
        state: &'a RichtextState,
        last_entity_index_cache: Option<ConverterCache>,
    }

    struct ConverterCache {
        entity_index: usize,
        cursor: Cursor,
        event_index: usize,
        cursor_elem_len: usize,
    }

    impl<'a> ContinuousIndexConverter<'a> {
        pub fn new(state: &'a RichtextState) -> Self {
            Self {
                state,
                last_entity_index_cache: None,
            }
        }

        pub fn convert_entity_index_to_event_index(&mut self, entity_index: usize) -> usize {
            self.state.check_cache();
            let result = {
                if let Some(last) = self.last_entity_index_cache.as_ref() {
                    if last.entity_index == entity_index {
                        return last.event_index;
                    }

                    assert!(entity_index > last.entity_index);
                    if last.cursor.offset + entity_index - last.entity_index < last.cursor_elem_len
                    {
                        // in the same cursor
                        return self.state.cursor_to_event_index(Cursor {
                            leaf: last.cursor.leaf,
                            offset: last.cursor.offset + entity_index - last.entity_index,
                        });
                    }
                }

                let cursor = self
                    .state
                    .tree
                    .query::<EntityQuery>(&entity_index)
                    .unwrap()
                    .cursor;
                let ans = self.state.cursor_to_event_index(cursor);
                let len = self.state.tree.get_elem(cursor.leaf).unwrap().rle_len();
                self.last_entity_index_cache = Some(ConverterCache {
                    entity_index,
                    cursor,
                    event_index: ans,
                    cursor_elem_len: len,
                });
                ans
            };
            self.state.check_cache();
            result
        }
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
                let (entity_index, _) = state
                    .get_entity_index_for_text_insert(pos, PosType::Unicode)
                    .unwrap();
                state.insert_at_entity_index(entity_index, text, IdFull::new(0, 0, 0));
            };
        }

        fn delete(&mut self, pos: usize, len: usize) {
            let ranges = self
                .state
                .get_text_entity_ranges(pos, len, PosType::Unicode)
                .unwrap();
            for range in ranges.into_iter().rev() {
                self.state.drain_by_entity_index(
                    range.entity_start,
                    range.entity_end - range.entity_start,
                    None,
                );
            }
        }

        fn mark(&mut self, range: Range<usize>, style: Arc<StyleOp>) {
            let (start, _) = self
                .state
                .get_entity_index_for_text_insert(range.start, PosType::Unicode)
                .unwrap();
            let (end, _) = self
                .state
                .get_entity_index_for_text_insert(range.end, PosType::Unicode)
                .unwrap();
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
            &format!("comment:{}", n),
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
        assert_eq!(wrapper.state.len_unicode(), 12);
        assert_eq!(wrapper.state.len_entity(), 12);
        wrapper.delete(0, 5);
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": " World!"
                }
            ])
        );

        assert_eq!(wrapper.state.len_unicode(), 7);
        assert_eq!(wrapper.state.len_entity(), 7);
        wrapper.delete(1, 1);

        assert_eq!(wrapper.state.len_unicode(), 6);
        assert_eq!(wrapper.state.len_entity(), 6);
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": " orld!"
                }
            ])
        );

        wrapper.delete(5, 1);
        assert_eq!(wrapper.state.len_unicode(), 5);
        assert_eq!(wrapper.state.len_entity(), 5);
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([
                {
                    "insert": " orld"
                }
            ])
        );

        wrapper.delete(0, 5);
        assert_eq!(wrapper.state.len_unicode(), 0);
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
                    "insert": " World!"
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
                        "comment:0": "comment",
                    },
                },
                {
                    "insert": "ello",
                    "attributes": {
                        "comment:0": "comment",
                        "comment:1": "comment",
                    },
                },

                {
                    "insert": " ",
                    "attributes": {
                        "comment:1": "comment",
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
        wrapper.state.drain_by_entity_index(
            0,
            7,
            Some(&mut |span| {
                if matches!(span, RichtextStateChunk::Style { .. }) {
                    count += 1;
                }
            }),
        );

        assert_eq!(count, 2);
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([{
                "insert": " World!"
            }])
        );
    }

    #[test]
    fn remove_start_anchor_should_remove_style() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello World!");
        wrapper.mark(0..5, bold(0));
        wrapper.state.drain_by_entity_index(6, 1, None);
        wrapper.state.drain_by_entity_index(0, 1, None);
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([{
                "insert": "Hello World!"
            }])
        );
    }

    #[test]
    fn remove_start_anchor_in_the_middle_should_remove_style() {
        let mut wrapper = SimpleWrapper::default();
        wrapper.insert(0, "Hello World!");
        wrapper.mark(2..5, bold(0));
        wrapper.state.drain_by_entity_index(6, 1, None);
        wrapper.state.drain_by_entity_index(1, 2, None);
        assert_eq!(
            wrapper.state.get_richtext_value().to_json_value(),
            json!([{
                "insert": "Hllo World!"
            }])
        );
    }
}
