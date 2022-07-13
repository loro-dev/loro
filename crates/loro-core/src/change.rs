use std::char::MAX;

use crate::{id::ID, op::Op};
use rle::{HasLength, Mergable, RleVec};

pub type Timestamp = i64;
const MAX_CHANGE_LENGTH: usize = 256;
const MAX_MERGABLE_INTERVAL: Timestamp = 256;
pub struct Change {
    pub(crate) ops: RleVec<Op>,
    pub(crate) id: ID,
    pub(crate) timestamp: Timestamp,
    /// Imported elements should be freezed, i.e. it cannot be merged with incoming changes.
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

        if self.len() > MAX_CHANGE_LENGTH {
            return false;
        }

        if other.timestamp - self.timestamp > MAX_MERGABLE_INTERVAL {
            return false;
        }

        self.id.client_id == other.id.client_id
            && self.id.counter + self.len() as u32 == other.id.counter
    }
}
