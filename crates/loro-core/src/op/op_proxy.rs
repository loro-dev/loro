use std::{borrow::Cow, ops::Range};

use rle::{HasLength, Sliceable};

use crate::{
    container::ContainerID,
    id::Counter,
    span::{HasId, HasIdSpan, HasLamport},
    Change, Lamport, Op, OpContent, OpType, Timestamp, ID,
};

/// OpProxy represents a slice of an Op
pub struct OpProxy<'a> {
    change: &'a Change,
    op: &'a Op,
    /// offset of op in change.
    /// i.e. change.id.inc(op_offset) == op.start
    op_offset: u32,
    /// slice range of the op, op[slice_range]
    slice_range: Range<Counter>,
}

impl PartialEq for OpProxy<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.op.id == other.op.id && self.slice_range == other.slice_range
    }
}

impl<'a> HasId for OpProxy<'a> {
    fn id_start(&self) -> ID {
        ID::new(
            self.op.id.client_id,
            self.op.id.counter + self.slice_range.start,
        )
    }
}

impl<'a> HasLength for OpProxy<'a> {
    fn len(&self) -> usize {
        self.slice_range.content_len()
    }
}

impl<'a> HasLamport for OpProxy<'a> {
    fn lamport(&self) -> Lamport {
        self.change.lamport + self.op_offset + self.slice_range.start as u32
    }
}

impl Eq for OpProxy<'_> {}

impl PartialOrd for OpProxy<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let cmp = self.lamport().cmp(&other.lamport());
        if let std::cmp::Ordering::Equal = cmp {
            Some(self.op.id.client_id.cmp(&other.op.id.client_id))
        } else {
            Some(cmp)
        }
    }
}

impl Ord for OpProxy<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let cmp = self.lamport().cmp(&other.lamport());
        if let std::cmp::Ordering::Equal = cmp {
            self.op.id.client_id.cmp(&other.op.id.client_id)
        } else {
            cmp
        }
    }
}

impl<'a> OpProxy<'a> {
    pub fn new(
        change: &'a Change,
        op: &'a Op,
        op_offset: u32,
        range: Option<Range<Counter>>,
    ) -> Self {
        OpProxy {
            change,
            op,
            slice_range: if let Some(range) = range {
                range
            } else {
                0..op.len() as Counter
            },
            op_offset,
        }
    }

    pub fn offset(&self) -> u32 {
        self.op_offset
    }

    pub fn lamport(&self) -> Lamport {
        self.change.lamport + self.op.id.counter as Lamport - self.change.id.counter as Lamport
            + self.slice_range.start as Lamport
    }

    pub fn timestamp(&self) -> Timestamp {
        self.change.timestamp
    }

    pub fn op(&self) -> &Op {
        self.op
    }

    pub fn slice_range(&self) -> &Range<Counter> {
        &self.slice_range
    }

    pub(crate) fn content(&self) -> &OpContent {
        &self.op.content
    }

    pub(crate) fn content_sliced(&self) -> Cow<'_, OpContent> {
        if self.slice_range.start == self.op.id.counter
            && self.slice_range.end == self.op.id_end().counter
        {
            Cow::Borrowed(&self.op.content)
        } else {
            Cow::Owned(self.op.content.slice(
                self.slice_range.start as usize,
                self.slice_range.end as usize,
            ))
        }
    }

    pub fn op_type(&self) -> OpType {
        self.op.op_type()
    }

    pub fn container(&self) -> &ContainerID {
        self.op.container()
    }
}
