use std::fmt::Display;

use crate::{
    container::text::text_content::SliceRange,
    id::Counter,
    span::{HasCounter, HasCounterSpan, IdSpan},
    ContentType, InsertContentTrait, ID,
};
use rle::{
    rle_tree::{tree_trait::CumulateTreeTrait, BumpMode},
    HasLength, Mergable, Sliceable,
};

const MAX_CHILDREN_SIZE: usize = 16;
pub(super) type YSpanTreeTrait = CumulateTreeTrait<YSpan, MAX_CHILDREN_SIZE, BumpMode>;

/// 80 bytes
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YSpan {
    // 16 bytes
    pub id: ID,
    // 8 bytes
    pub status: Status,
    // 24 bytes
    pub origin_left: Option<ID>,
    // 24 bytes
    pub origin_right: Option<ID>,
    // 8 bytes
    pub slice: SliceRange,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub struct Status {
    /// is this span from a future operation
    pub future: bool,
    pub delete_times: u16,
    pub undo_times: u16,
}

impl Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_activated() {
            write!(f, "Active",)
        } else {
            write!(
                f,
                "unapplied: {}, delete_times: {}, undo_times: {}",
                self.future, self.delete_times, self.undo_times
            )
        }
    }
}

impl Status {
    #[inline]
    pub fn new() -> Self {
        Status {
            future: false,
            delete_times: 0,
            undo_times: 0,
        }
    }

    #[inline]
    pub fn is_activated(&self) -> bool {
        !self.future && self.delete_times == 0 && self.undo_times == 0
    }

    /// Return whether the activation changed
    #[inline]
    pub fn apply(&mut self, change: StatusChange) -> bool {
        let activated = self.is_activated();
        match change {
            StatusChange::SetAsCurrent => self.future = false,
            StatusChange::SetAsFuture => self.future = true,
            StatusChange::Redo => self.undo_times -= 1,
            StatusChange::Undo => self.undo_times += 1,
            StatusChange::Delete => self.delete_times += 1,
            StatusChange::UndoDelete => self.delete_times -= 1,
        }

        self.is_activated() != activated
    }
}

#[test]
fn y_span_size() {
    println!("{}", std::mem::size_of::<YSpan>());
}

#[derive(Clone, Copy, Debug)]
pub enum StatusChange {
    SetAsCurrent,
    SetAsFuture,
    Redo,
    Undo,
    Delete,
    UndoDelete,
}

impl YSpan {
    /// this is the last id of the span, which is **included** by self
    #[inline]
    pub fn last_id(&self) -> ID {
        self.id.inc(self.atom_len() as i32 - 1)
    }

    #[inline]
    pub fn can_be_origin(&self) -> bool {
        self.status.is_activated()
    }

    #[inline]
    pub fn contain_id(&self, id: ID) -> bool {
        self.id.client_id == id.client_id
            && self.id.counter <= id.counter
            && id.counter < self.id.counter + self.atom_len() as i32
    }

    #[inline]
    pub fn overlap(&self, id: IdSpan) -> bool {
        if self.id.client_id != id.client_id {
            return false;
        }

        self.id.counter < id.ctr_end()
            && self.id.counter + (self.atom_len() as Counter) > id.ctr_start()
    }
}

impl Mergable for YSpan {
    fn is_mergable(&self, other: &Self, _: &()) -> bool {
        other.id.client_id == self.id.client_id
            && self.status == other.status
            && self.id.counter + self.atom_len() as Counter == other.id.counter
            && self.origin_right == other.origin_right
            && Some(self.id.inc(self.atom_len() as Counter - 1)) == other.origin_left
            && self.slice.is_mergable(&other.slice, &())
    }

    fn merge(&mut self, other: &Self, _: &()) {
        self.origin_right = other.origin_right;
        self.slice.merge(&other.slice, &())
    }
}

impl Sliceable for YSpan {
    fn slice(&self, from: usize, to: usize) -> Self {
        if from == 0 && to == self.atom_len() {
            return self.clone();
        }

        let origin_left = if from == 0 {
            self.origin_left
        } else {
            Some(self.id.inc(from as i32 - 1))
        };

        // origin_right should be the same
        let origin_right = self.origin_right;
        YSpan {
            origin_left,
            origin_right,
            id: self.id.inc(from as i32),
            status: self.status.clone(),
            slice: self.slice.slice(from, to),
        }
    }
}

impl InsertContentTrait for YSpan {
    fn id(&self) -> ContentType {
        ContentType::List
    }
}

impl HasLength for YSpan {
    #[inline(always)]
    fn content_len(&self) -> usize {
        if self.status.is_activated() {
            self.slice.atom_len()
        } else {
            0
        }
    }

    #[inline(always)]
    fn atom_len(&self) -> usize {
        self.slice.atom_len()
    }
}
