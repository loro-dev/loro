use crate::{Change, Lamport, Op, Timestamp, ID};

pub struct OpProxy<'a> {
    change: &'a Change,
    op: &'a Op,
    /// offset of op in change
    offset: u32,
}

impl<'a> OpProxy<'a> {
    pub fn new(change: &'a Change, op: &'a Op) -> Self {
        OpProxy {
            change,
            op,
            offset: 0,
        }
    }

    pub fn lamport(&self) -> Lamport {
        self.change.lamport + self.offset
    }

    pub fn id(&self) -> ID {
        self.op.id
    }

    pub fn timestamp(&self) -> Timestamp {
        self.change.timestamp
    }
}
