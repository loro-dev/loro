use rle::{HasLength, Mergable, RleVec};

use crate::{id::ID, op::Op};

pub(crate) struct Change {
    pub(crate) ops: RleVec<Op>,
    pub(crate) id: ID,
    pub(crate) timestamp: i64,
    pub(crate) freezed: bool,
}

impl Change {
    pub(crate) fn new(id: ID, timestamp: i64, ops: RleVec<Op>, freezed: bool) -> Self {
        Change {
            id,
            timestamp,
            ops,
            freezed,
        }
    }
}

impl HasLength for Change {
    fn len(&self) -> usize {
        self.ops.len()
    }
}

impl Mergable for Change {
    fn merge(&mut self, other: &Self) {
        self.ops.merge(&other.ops);
    }

    fn is_mergable(&self, other: &Self) -> bool {
        if self.freezed {
            return false;
        }

        self.id.client_id == other.id.client_id
            && self.id.counter + self.len() as u32 == other.id.counter
    }
}
