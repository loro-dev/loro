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

use loro_common::{Counter, LoroValue, PeerID, ID};
use std::{
    borrow::Cow,
    ops::{Range, RangeBounds},
};

use crate::{change::Lamport, InternalString, VersionVector};

use super::list::list_op::ListOp;

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
#[derive(
    Default, Clone, Copy, Eq, PartialEq, Debug, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct TextStyleInfo {
    data: u8,
}

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
    pub fn mergeable(&self) -> bool {
        self.data & 0b0000_0001 != 0
    }

    /// When inserting new text around this style, prefer inserting after it.
    #[inline(always)]
    pub fn expand_before(&self) -> bool {
        self.data & 0b0000_0010 != 0
    }

    /// When inserting new text around this style, prefer inserting before it.
    #[inline(always)]
    pub fn expand_after(&self) -> bool {
        self.data & 0b0000_0100 != 0
    }

    #[inline(always)]
    pub fn anchor_type(&self) -> AnchorType {
        if self.data & 0b0000_1000 != 0 {
            AnchorType::End
        } else {
            AnchorType::Start
        }
    }

    #[inline(always)]
    pub fn is_delete(&self) -> bool {
        self.data & 0b0001_0000 != 0
    }

    #[inline(always)]
    pub fn is_container(&self) -> bool {
        self.data & 0b0010_0000 != 0
    }

    pub const fn new(
        mergeable: bool,
        expand_type: ExpandType,
        anchor_type: AnchorType,
        is_delete: bool,
        is_container: bool,
    ) -> Self {
        let mut data = 1;
        if mergeable {
            data |= 0b0000_0001;
        }
        if expand_type.expand_before() {
            data |= 0b0000_0010;
        }
        if expand_type.expand_after() {
            data |= 0b0000_0100;
        }
        if matches!(anchor_type, AnchorType::End) {
            data |= 0b0000_1000;
        }
        if is_delete {
            data |= 0b0001_0000;
        }
        if is_container {
            data |= 0b0010_0000;
        }

        TextStyleInfo { data }
    }

    pub const fn is_dead(self) -> bool {
        debug_assert!((self.data & 1 != 0) || self.data == 0);
        (self.data & 1) == 0
    }

    #[inline(always)]
    pub const fn to_delete(self) -> Self {
        let mut data = self.data;
        // set is_delete
        data |= 0b0001_0000;
        // invert expand type
        data ^= 0b0000_0110;
        Self { data }
    }

    #[inline(always)]
    pub const fn to_end(self) -> Self {
        Self {
            // set is_end
            data: self.data | 0b0000_1000,
        }
    }

    pub const BOLD: TextStyleInfo =
        TextStyleInfo::new(true, ExpandType::After, AnchorType::Start, false, false);
    pub const LINK: TextStyleInfo =
        TextStyleInfo::new(true, ExpandType::None, AnchorType::Start, false, false);
    pub const COMMENT: TextStyleInfo =
        TextStyleInfo::new(true, ExpandType::None, AnchorType::Start, false, true);
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test() {}
}
