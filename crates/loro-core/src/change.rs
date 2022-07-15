use std::char::MAX;

use crate::{id::ID, op::Op};
use rle::{HasLength, Mergable, RleVec};
use smallvec::SmallVec;

pub type Timestamp = i64;
pub type Lamport = u32;

/// A `Change` contains a list of [Op]s.
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

pub struct ChangeMergeCfg {
    pub max_change_length: usize,
    pub max_change_interval: usize,
}

impl Default for ChangeMergeCfg {
    fn default() -> Self {
        ChangeMergeCfg {
            max_change_length: 1024,
            max_change_interval: 60,
        }
    }
}

impl Mergable<ChangeMergeCfg> for Change {
    fn merge(&mut self, other: &Self, _: &ChangeMergeCfg) {
        self.ops.merge(&other.ops, &());
    }

    fn is_mergable(&self, other: &Self, cfg: &ChangeMergeCfg) -> bool {
        if self.freezed {
            return false;
        }

        if !other.deps.is_empty() {
            return false;
        }

        if self.len() > cfg.max_change_length {
            return false;
        }

        if other.timestamp - self.timestamp > cfg.max_change_interval as i64 {
            return false;
        }

        self.id.client_id == other.id.client_id
            && self.id.counter + self.len() as u32 == other.id.counter
            && self.lamport + self.len() as Lamport == other.lamport
    }
}
