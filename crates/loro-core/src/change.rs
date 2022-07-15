use std::char::MAX;

use crate::{id::ID, op::Op};
use rle::{HasLength, Mergable, RleVec};
use smallvec::SmallVec;

pub type Timestamp = i64;
pub type Lamport = u64;
const MAX_CHANGE_LENGTH: usize = 256;
/// TODO: Should this be configurable?
const MAX_MERGABLE_INTERVAL: Timestamp = 60;

/// Change
#[derive(Debug)]
pub struct Change {
    pub(crate) ops: RleVec<Op>,
    pub(crate) deps: SmallVec<[ID; 2]>,
    /// id of the first op in the change
    pub(crate) id: ID,
    /// Lamport timestamp of the change. It can be calculated from deps
    pub(crate) lamport: Lamport,
    /// [Unix time](https://en.wikipedia.org/wiki/Unix_time)
    /// It is the number of seconds that have elapsed since 00:00:00 UTC on 1 January 1970.
    pub(crate) timestamp: Timestamp,
    /// Whether this change can be merged with the next change
    /// - Only the last change in a chain can be merged with the next change
    /// - Imported changes should be freezed
    pub(crate) freezed: bool,
}

impl Change {
    pub fn new(
        ops: RleVec<Op>,
        deps: SmallVec<[ID; 2]>,
        id: ID,
        lamport: Lamport,
        timestamp: Timestamp,
        freezed: bool,
    ) -> Self {
        Change {
            ops,
            deps,
            id,
            lamport,
            timestamp,
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

        if !other.deps.is_empty() {
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
