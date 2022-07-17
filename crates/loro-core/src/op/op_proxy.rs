use std::ops::Range;

use rle::{HasLength, Sliceable};

use crate::{container::ContainerID, Change, Lamport, Op, OpContent, OpType, Timestamp, ID};

/// OpProxy represents a slice of an Op
pub struct OpProxy<'a> {
    change: &'a Change,
    op: &'a Op,
    slice_range: Range<u32>,
}

impl<'a> OpProxy<'a> {
    pub fn new(change: &'a Change, op: &'a Op, range: Option<Range<u32>>) -> Self {
        OpProxy {
            change,
            op,
            slice_range: if let Some(range) = range {
                range
            } else {
                op.id.counter..op.id.counter + op.len() as u32
            },
        }
    }

    pub fn lamport(&self) -> Lamport {
        self.change.lamport + self.op.id.counter - self.change.id.counter + self.slice_range.start
    }

    pub fn id(&self) -> ID {
        ID::new(
            self.op.id.client_id,
            self.op.id.counter + self.slice_range.start,
        )
    }

    pub fn timestamp(&self) -> Timestamp {
        self.change.timestamp
    }

    pub fn op(&self) -> &Op {
        self.op
    }

    pub fn slice_range(&self) -> &Range<u32> {
        &self.slice_range
    }

    pub fn content(&self) -> &OpContent {
        &self.op.content
    }

    pub fn content_sliced(&self) -> OpContent {
        self.op.content.slice(
            self.slice_range.start as usize,
            self.slice_range.end as usize,
        )
    }

    pub fn op_type(&self) -> OpType {
        self.op.op_type()
    }

    pub fn container(&self) -> &ContainerID {
        self.op.container()
    }
}
