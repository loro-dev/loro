//! # Index
//!
//! There are several types of indexes:
//!
//! - Unicode index: the index of a unicode code point in the text.
//! - Entity index: unicode index + style anchor index. Each unicode code point or style anchor is an entity.
//! - Utf16 index
//!
//! In [crate::op::Op], we always use entity index to persist richtext ops.
//!
//! The users of this type can only operate on unicode index or utf16 index, but calculated entity index will be provided.

mod query_by_len;
mod richtext_state;
mod style_range_map;
mod tinyvec;
mod tracker;

use loro_common::{Counter, LoroValue, PeerID};
use std::{borrow::Cow, fmt::Debug};

use crate::{change::Lamport, InternalString};

/// This is the data structure that represents a span of rich text.
/// It's used to communicate with the frontend.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RichtextSpan<'a> {
    pub text: Cow<'a, str>,
    pub styles: Vec<Style>,
}

/// This is used to communicate with the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Style {
    pub key: InternalString,
    /// The value of the style.
    ///
    /// - If the style is a container, this is the Container
    /// - Otherwise, this is null
    pub data: LoroValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct StyleInner {
    pub(crate) lamport: Lamport,
    pub(crate) peer: PeerID,
    pub(crate) cnt: Counter,
    pub(crate) key: InternalString,
    pub(crate) info: TextStyleInfo,
}

impl StyleInner {
    pub fn to_style(&self) -> Option<Style> {
        if self.info.is_delete() {
            return None;
        }

        if self.info.is_container() {
            Some(Style {
                key: self.key.clone(),
                data: LoroValue::Container(loro_common::ContainerID::Normal {
                    peer: self.peer,
                    counter: self.cnt,
                    container_type: loro_common::ContainerType::Map,
                }),
            })
        } else {
            Some(Style {
                key: self.key.clone(),
                data: LoroValue::Null,
            })
        }
    }

    #[cfg(test)]
    pub fn new_for_test(n: isize, key: &str, info: TextStyleInfo) -> Self {
        Self {
            lamport: n as Lamport,
            peer: n as PeerID,
            cnt: n as Counter,
            key: key.to_string().into(),
            info,
        }
    }
}

impl PartialOrd for StyleInner {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for StyleInner {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.lamport
            .cmp(&other.lamport)
            .then(self.peer.cmp(&other.peer))
    }
}

/// A compact representation of a rich text style config.
///
/// Note: we assume style with the same key has the same `Mergeable` and `isContainer` value.
///
/// - Mergeable      (1st bit): whether two styles with the same key can be merged into one.
/// - Expand Before  (2nd bit): when inserting new text before this style, whether the new text should inherit this style.
/// - Expand After   (3rd bit): when inserting new text after  this style, whether the new text should inherit this style.
/// - isEnd          (4th bit): whether this is a begin style anchor or an end style anchor.
///                             This is only used to describe styles anchors. When it's used to describe a style, this bit is always 0.
/// - Delete         (5th bit): whether this is used to remove a style from a range.
/// - isContainer    (6th bit): whether the style also store other data in a associated map container with the same OpID.
/// - 0              (7th bit)
/// - isAlive        (8th bit): always 1 unless the style is garbage collected. If this is 0, all other bits should be 0 as well.
#[derive(Default, Clone, Copy, Eq, PartialEq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TextStyleInfo {
    data: u8,
}

impl Debug for TextStyleInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextStyleInfo")
            // write data in binary format
            .field("data", &format!("{:#010b}", self.data))
            .field("mergeable", &self.mergeable())
            .field("expand_before", &self.expand_before())
            .field("expand_after", &self.expand_after())
            .field("is_end", &self.is_end())
            .field("is_delete", &self.is_delete())
            .field("is_container", &self.is_container())
            .finish()
    }
}

const MERGEABLE_MASK: u8 = 0b0000_0001;
const EXPAND_BEFORE_MASK: u8 = 0b0000_0010;
const EXPAND_AFTER_MASK: u8 = 0b0000_0100;
const IS_END_MASK: u8 = 0b0000_1000;
const DELETE_MASK: u8 = 0b0001_0000;
const CONTAINER_MASK: u8 = 0b0010_0000;
const ALIVE_MASK: u8 = 0b1000_0000;

#[derive(Clone, Copy, Eq, PartialEq, Debug, Hash)]
pub enum ExpandType {
    Before,
    After,
    Both,
    None,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum AnchorType {
    Start,
    End,
}

impl ExpandType {
    #[inline(always)]
    pub const fn expand_before(&self) -> bool {
        matches!(self, ExpandType::Before | ExpandType::Both)
    }

    #[inline(always)]
    pub const fn expand_after(&self) -> bool {
        matches!(self, ExpandType::After | ExpandType::Both)
    }
}

impl TextStyleInfo {
    /// Whether two styles with the same key can be merged into one.
    /// If false, the styles will coexist in the same range.
    #[inline(always)]
    pub fn mergeable(self) -> bool {
        self.data & MERGEABLE_MASK != 0
    }

    /// When inserting new text around this style, prefer inserting after it.
    #[inline(always)]
    pub fn expand_before(self) -> bool {
        self.data & EXPAND_BEFORE_MASK != 0
    }

    /// When inserting new text around this style, prefer inserting before it.
    #[inline(always)]
    pub fn expand_after(self) -> bool {
        self.data & EXPAND_AFTER_MASK != 0
    }

    #[inline(always)]
    pub fn anchor_type(self) -> AnchorType {
        if self.data & IS_END_MASK != 0 {
            AnchorType::End
        } else {
            AnchorType::Start
        }
    }

    #[inline(always)]
    pub fn is_end(self) -> bool {
        self.data & IS_END_MASK != 0
    }

    #[inline(always)]
    pub fn is_start(self) -> bool {
        self.data & IS_END_MASK == 0
    }

    /// This method tells that when we can insert text before/after this style anchor, whether we insert the new text before the anchor.
    #[inline]
    pub fn prefer_insert_before(self) -> bool {
        if self.is_end() {
            // If we need to expand the style, the new text should be inserted **before** the end anchor
            self.expand_after()
        } else {
            // If we need to expand the style, the new text should be inserted **after** the start anchor
            !self.expand_before()
        }
    }

    #[inline(always)]
    pub fn is_delete(&self) -> bool {
        self.data & DELETE_MASK != 0
    }

    #[inline(always)]
    pub fn is_container(&self) -> bool {
        self.data & CONTAINER_MASK != 0
    }

    pub const fn new(
        mergeable: bool,
        expand_type: ExpandType,
        anchor_type: AnchorType,
        is_delete: bool,
        is_container: bool,
    ) -> Self {
        let mut data = ALIVE_MASK;
        if mergeable {
            data |= MERGEABLE_MASK;
        }
        if expand_type.expand_before() {
            data |= EXPAND_BEFORE_MASK;
        }
        if expand_type.expand_after() {
            data |= EXPAND_AFTER_MASK;
        }
        if matches!(anchor_type, AnchorType::End) {
            data |= IS_END_MASK;
        }
        if is_delete {
            data |= DELETE_MASK;
        }
        if is_container {
            data |= CONTAINER_MASK;
        }

        TextStyleInfo { data }
    }

    pub const fn is_dead(self) -> bool {
        debug_assert!((self.data & ALIVE_MASK != 0) || self.data == 0);
        (self.data & ALIVE_MASK) == 0
    }

    #[inline(always)]
    pub const fn to_delete(self) -> Self {
        let mut data = self.data;
        if data & DELETE_MASK > 0 {
            return Self { data };
        }

        // set is_delete
        data |= DELETE_MASK;
        // invert expand type
        data ^= EXPAND_AFTER_MASK | EXPAND_BEFORE_MASK;
        Self { data }
    }

    #[inline(always)]
    pub const fn to_end(self) -> Self {
        Self {
            // set is_end
            data: self.data | IS_END_MASK,
        }
    }

    #[inline(always)]
    pub const fn to_start(self) -> Self {
        Self {
            // set is_end
            data: self.data & (!IS_END_MASK),
        }
    }

    pub const BOLD: TextStyleInfo =
        TextStyleInfo::new(true, ExpandType::After, AnchorType::Start, false, false);
    pub const LINK: TextStyleInfo =
        TextStyleInfo::new(true, ExpandType::None, AnchorType::Start, false, false);
    pub const COMMENT: TextStyleInfo =
        TextStyleInfo::new(false, ExpandType::None, AnchorType::Start, false, true);
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test() {}
}
