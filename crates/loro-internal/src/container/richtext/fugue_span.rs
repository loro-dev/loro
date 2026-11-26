use std::ops::Range;

use generic_btree::rle::{CanRemove, HasLength, Mergeable, Sliceable, TryInsert};
use loro_common::{CompactId, Counter, HasId, IdFull, IdSpan, Lamport, ID};
use serde::{Deserialize, Serialize};

use super::{tracker::UNKNOWN_PEER_ID, AnchorType};

#[derive(Clone, PartialEq, Eq, Copy, Serialize, Deserialize)]
pub(crate) struct RichtextChunk {
    start: u32,
    end: u32,
}

impl std::fmt::Debug for RichtextChunk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RichtextChunk")
            .field("value", &self.value())
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub(crate) enum RichtextChunkKind {
    Text,
    StyleAnchor,
    Unknown,
    /// Move anchor is a special anchor that is used to mark the FugueSpan created
    /// by a move operation.
    MoveAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RichtextChunkValue {
    Text(Range<u32>),
    StyleAnchor {
        id: u32,
        anchor_type: AnchorType,
    },
    Unknown(u32),
    /// Move anchor is a special anchor that is used to mark the FugueSpan created
    /// by a move operation.
    MoveAnchor,
}

impl RichtextChunk {
    pub(crate) const UNKNOWN: u32 = u32::MAX;
    pub(crate) const START_STYLE_ANCHOR: u32 = u32::MAX - 1;
    pub(crate) const END_STYLE_ANCHOR: u32 = u32::MAX - 2;
    pub(crate) const MOVE_ANCHOR: u32 = u32::MAX - 3;

    #[inline]
    pub fn new_text(range: Range<u32>) -> Self {
        Self {
            start: range.start,
            end: range.end,
        }
    }

    #[inline]
    pub fn new_style_anchor(idx: u32, anchor_type: AnchorType) -> Self {
        match anchor_type {
            AnchorType::Start => Self {
                start: Self::START_STYLE_ANCHOR,
                end: idx,
            },
            AnchorType::End => Self {
                start: Self::END_STYLE_ANCHOR,
                end: idx,
            },
        }
    }

    #[inline]
    pub fn new_unknown(len: u32) -> Self {
        Self {
            start: Self::UNKNOWN,
            end: len,
        }
    }

    #[inline]
    pub fn new_move() -> Self {
        Self {
            start: Self::MOVE_ANCHOR,
            end: Self::MOVE_ANCHOR,
        }
    }

    #[inline]
    pub(crate) fn kind(&self) -> RichtextChunkKind {
        match self.start {
            Self::START_STYLE_ANCHOR => RichtextChunkKind::StyleAnchor,
            Self::END_STYLE_ANCHOR => RichtextChunkKind::StyleAnchor,
            Self::UNKNOWN => RichtextChunkKind::Unknown,
            Self::MOVE_ANCHOR => RichtextChunkKind::MoveAnchor,
            _ => RichtextChunkKind::Text,
        }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        match self.start {
            Self::UNKNOWN => self.end as usize,
            Self::START_STYLE_ANCHOR | Self::END_STYLE_ANCHOR | Self::MOVE_ANCHOR => 1,
            _ => (self.end - self.start) as usize,
        }
    }

    #[inline]
    pub(crate) fn value(&self) -> RichtextChunkValue {
        match self.start {
            Self::UNKNOWN => RichtextChunkValue::Unknown(self.end),
            Self::START_STYLE_ANCHOR => RichtextChunkValue::StyleAnchor {
                id: self.end,
                anchor_type: AnchorType::Start,
            },
            Self::END_STYLE_ANCHOR => RichtextChunkValue::StyleAnchor {
                id: self.end,
                anchor_type: AnchorType::End,
            },
            Self::MOVE_ANCHOR => RichtextChunkValue::MoveAnchor,
            _ => RichtextChunkValue::Text(self.start..self.end),
        }
    }
}

impl Mergeable for RichtextChunk {
    fn can_merge(&self, rhs: &Self) -> bool {
        match (self.kind(), rhs.kind()) {
            (RichtextChunkKind::Text, RichtextChunkKind::Text) => self.end == rhs.start,
            (RichtextChunkKind::Unknown, RichtextChunkKind::Unknown) => true,
            _ => false,
        }
    }

    fn merge_right(&mut self, rhs: &Self) {
        match (self.kind(), rhs.kind()) {
            (RichtextChunkKind::Text, RichtextChunkKind::Text) => self.end = rhs.end,
            (RichtextChunkKind::Unknown, RichtextChunkKind::Unknown) => self.end += rhs.end,
            _ => unreachable!(),
        }
    }

    fn merge_left(&mut self, left: &Self) {
        match (self.kind(), left.kind()) {
            (RichtextChunkKind::Text, RichtextChunkKind::Text) => self.start = left.start,
            (RichtextChunkKind::Unknown, RichtextChunkKind::Unknown) => self.end += left.end,
            _ => unreachable!(),
        }
    }
}

impl HasLength for RichtextChunk {
    #[inline(always)]
    fn rle_len(&self) -> usize {
        self.len()
    }
}

impl Sliceable for RichtextChunk {
    fn _slice(&self, range: Range<usize>) -> Self {
        match self.kind() {
            RichtextChunkKind::Text => {
                assert!(
                    range.len() <= self.len(),
                    "range: {:?}, self: {:?}",
                    range,
                    self
                );
                Self {
                    start: self.start + range.start as u32,
                    end: self.start + range.end as u32,
                }
            }
            RichtextChunkKind::StyleAnchor | RichtextChunkKind::MoveAnchor => {
                assert_eq!(range.len(), 1);
                *self
            }
            RichtextChunkKind::Unknown => {
                assert!(range.len() <= self.len());
                Self {
                    start: Self::UNKNOWN,
                    end: range.len() as u32,
                }
            }
        }
    }
}

/// Sentinel stored in `FugueSpan::id.lamport` for `UNKNOWN_PEER_ID` spans whose
/// `real_id` has not yet been revealed by a deletion. Once deleted, `id.lamport`
/// holds the index into `CrdtRope::unknown_real_ids`.
pub(super) const UNKNOWN_REAL_ID_UNSET: Lamport = u32::MAX;

/// Compact 4-byte representation of `Option<Status>`, avoiding the 2-byte tag +
/// alignment padding of `Option<Status>` inside `FugueSpan`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Hash, Copy)]
pub(super) struct OptionalStatus {
    has_value: bool,
    future: bool,
    delete_times: i16,
}

impl OptionalStatus {
    pub const NONE: Self = Self {
        has_value: false,
        future: false,
        delete_times: 0,
    };

    #[inline(always)]
    pub fn some(status: Status) -> Self {
        Self {
            has_value: true,
            future: status.future,
            delete_times: status.delete_times,
        }
    }

    #[inline(always)]
    pub fn is_some(self) -> bool {
        self.has_value
    }

    #[inline(always)]
    pub fn get(self) -> Option<Status> {
        self.has_value.then_some(Status {
            future: self.future,
            delete_times: self.delete_times,
        })
    }

    #[inline(always)]
    pub fn get_or_insert(&mut self, default: Status) -> StatusMut<'_> {
        if !self.has_value {
            *self = Self::some(default);
        }
        StatusMut {
            future: &mut self.future,
            delete_times: &mut self.delete_times,
        }
    }
}

pub(super) struct StatusMut<'a> {
    pub future: &'a mut bool,
    pub delete_times: &'a mut i16,
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub(super) struct FugueSpan {
    pub id: IdFull,
    pub origin_left: Option<CompactId>,
    pub origin_right: Option<CompactId>,
    pub content: RichtextChunk,
    /// The status at the current version
    pub status: Status,
    /// The status at the `new` version.
    /// It's used when calculating diff.
    pub diff_status: OptionalStatus,
}

// Bound the hot-path memory footprint without requiring identical Rust layouts:
// targets with 4-byte alignment for u64 (for example i686 musl) use fewer bytes.
const _: () = assert!(std::mem::size_of::<FugueSpan>() <= 64);

pub(super) enum DiffStatus {
    NotChanged,
    Created,
    Deleted,
}

impl FugueSpan {
    #[inline(always)]
    pub fn id_span(&self) -> IdSpan {
        IdSpan::new(
            self.id.peer,
            self.id.counter,
            self.id.counter + self.content.len() as Counter,
        )
    }

    #[inline]
    pub fn diff(&self) -> DiffStatus {
        let Some(diff_status) = self.diff_status.get() else {
            return DiffStatus::NotChanged;
        };

        match (self.status.is_activated(), diff_status.is_activated()) {
            (true, false) => DiffStatus::Deleted,
            (false, true) => DiffStatus::Created,
            _ => DiffStatus::NotChanged,
        }
    }

    pub fn is_activated_in_diff(&self) -> bool {
        match self.diff_status.get() {
            Some(d) => d.is_activated(),
            None => self.status.is_activated(),
        }
    }
}

impl Sliceable for FugueSpan {
    fn _slice(&self, range: Range<usize>) -> Self {
        let id = if self.id.peer == UNKNOWN_PEER_ID {
            IdFull::new(
                UNKNOWN_PEER_ID,
                self.id.counter + range.start as Counter,
                self.id.lamport,
            )
        } else {
            self.id.inc(range.start as Counter)
        };
        Self {
            id,
            status: self.status,
            diff_status: self.diff_status,
            origin_left: if range.start == 0 {
                self.origin_left
            } else {
                Some(
                    ID::new(self.id.peer, self.id.counter + (range.start - 1) as Counter)
                        .try_into()
                        .unwrap(),
                )
            },
            origin_right: self.origin_right,
            content: self.content._slice(range),
        }
    }
}

impl HasLength for FugueSpan {
    #[inline(always)]
    fn rle_len(&self) -> usize {
        self.content.len()
    }
}

impl Mergeable for FugueSpan {
    fn can_merge(&self, rhs: &Self) -> bool {
        self.id.peer == rhs.id.peer
            && self.status == rhs.status
            && self.diff_status == rhs.diff_status
            && self.id.counter + self.content.len() as Counter == rhs.id.counter
            && if self.id.peer == UNKNOWN_PEER_ID {
                self.id.lamport == rhs.id.lamport
            } else {
                self.id.lamport + self.content.len() as Lamport == rhs.id.lamport
            }
            && rhs.origin_left.is_some()
            && rhs.origin_left.unwrap().peer == self.id.peer
            && rhs.origin_left.unwrap().counter.get()
                == self.id.counter + self.content.len() as Counter - 1
            && self.origin_right == rhs.origin_right
            && self.content.can_merge(&rhs.content)
    }

    fn merge_right(&mut self, rhs: &Self) {
        self.content.merge_right(&rhs.content);
    }

    fn merge_left(&mut self, left: &Self) {
        self.id = left.id;
        self.origin_left = left.origin_left;
        self.content.merge_left(&left.content);
    }
}

impl TryInsert for FugueSpan {
    fn try_insert(&mut self, _pos: usize, elem: Self) -> Result<(), Self>
    where
        Self: Sized,
    {
        Err(elem)
    }
}

impl CanRemove for FugueSpan {
    fn can_remove(&self) -> bool {
        self.content.len() == 0
    }
}

impl FugueSpan {
    #[allow(unused)]
    pub fn new(mut id: IdFull, content: RichtextChunk) -> Self {
        if id.peer == UNKNOWN_PEER_ID {
            id.lamport = UNKNOWN_REAL_ID_UNSET;
        }
        Self {
            id,
            status: Status::default(),
            diff_status: OptionalStatus::NONE,
            origin_left: None,
            origin_right: None,
            content,
        }
    }

    #[inline(always)]
    pub fn is_activated(&self) -> bool {
        self.status.is_activated()
    }

    #[inline]
    pub fn activated_len(&self) -> usize {
        if self.is_activated() {
            self.content.len()
        } else {
            0
        }
    }
}

impl HasId for FugueSpan {
    fn id_start(&self) -> ID {
        self.id.id()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Hash, Copy)]
pub(super) struct Status {
    /// is this span from a future operation
    pub future: bool,
    pub delete_times: i16,
}

impl Status {
    #[inline(always)]
    pub fn is_activated(&self) -> bool {
        self.delete_times == 0 && !self.future
    }
}
