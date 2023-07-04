//! [Change]s are merged ops.
//!
//! Every [Change] has deps on other [Change]s. All [Change]s in the document thus form a DAG.
//! Note, `dep` may point to the middle of the other [Change].
//!
//! In future, we may also use [Change] to represent a transaction. But this decision is postponed.

use crate::{
    dag::DagNode,
    id::{Counter, ID},
    op::Op,
    span::{HasId, HasIdSpan, HasLamport},
    version::Frontiers,
};
use num::traits::AsPrimitive;
use rle::{HasIndex, HasLength, Mergable, Rle, RleVec, Sliceable};

pub type Timestamp = i64;
pub type Lamport = u32;

/// A `Change` contains a list of [Op]s.
///
/// When undo/redo we should always undo/redo a whole [Change].
#[derive(Debug, Clone)]
pub struct Change<O = Op> {
    pub(crate) ops: RleVec<[O; 2]>,
    pub(crate) deps: Frontiers,
    /// id of the first op in the change
    pub(crate) id: ID,
    /// Lamport timestamp of the change. It can be calculated from deps
    pub(crate) lamport: Lamport,
    /// [Unix time](https://en.wikipedia.org/wiki/Unix_time)
    /// It is the number of seconds that have elapsed since 00:00:00 UTC on 1 January 1970.
    pub(crate) timestamp: Timestamp,
}

impl<O> Change<O> {
    pub fn new(
        ops: RleVec<[O; 2]>,
        deps: Frontiers,
        id: ID,
        lamport: Lamport,
        timestamp: Timestamp,
    ) -> Self {
        Change {
            ops,
            deps,
            id,
            lamport,
            timestamp,
        }
    }
}

impl<O> HasId for Change<O> {
    fn id_start(&self) -> ID {
        self.id
    }
}

impl<O> HasLamport for Change<O> {
    fn lamport(&self) -> Lamport {
        self.lamport
    }
}
use std::fmt::Debug;
impl<O: Mergable + HasLength + HasIndex + Debug> HasLength for Change<O> {
    fn content_len(&self) -> usize {
        self.ops.span().as_()
    }
}

#[derive(Debug, Clone)]
pub struct ChangeMergeCfg {
    pub max_change_length: usize,
    pub max_change_interval: usize,
}

impl ChangeMergeCfg {
    pub fn new() -> Self {
        ChangeMergeCfg {
            max_change_length: 1024,
            max_change_interval: 60,
        }
    }
}

impl Default for ChangeMergeCfg {
    fn default() -> Self {
        Self {
            max_change_length: 1024,
            max_change_interval: 60,
        }
    }
}

impl<O: Rle + HasIndex> Mergable<ChangeMergeCfg> for Change<O> {
    fn merge(&mut self, other: &Self, _: &ChangeMergeCfg) {
        self.ops.merge(&other.ops, &());
    }

    fn is_mergable(&self, other: &Self, cfg: &ChangeMergeCfg) -> bool {
        if other.deps.is_empty() || !(other.deps.len() == 1 && self.id_last() == other.deps[0]) {
            return false;
        }

        if self.content_len() > cfg.max_change_length {
            return false;
        }

        if other.timestamp - self.timestamp > cfg.max_change_interval as i64 {
            return false;
        }

        self.id.peer == other.id.peer
            && self.id.counter + self.content_len() as Counter == other.id.counter
            && self.lamport + self.content_len() as Lamport == other.lamport
    }
}

impl<O: Mergable + HasLength + Sliceable> Sliceable for Change<O> {
    // TODO: feels slow, need to confirm whether this affects performance
    fn slice(&self, from: usize, to: usize) -> Self {
        Self {
            ops: self.ops.slice(from, to),
            deps: if from > 0 {
                Frontiers::from_id(self.id.inc(from as Counter - 1))
            } else {
                self.deps.clone()
            },
            id: self.id.inc(from as Counter),
            lamport: self.lamport + from as Lamport,
            timestamp: self.timestamp,
        }
    }
}

impl DagNode for Change {
    fn deps(&self) -> &[ID] {
        &self.deps
    }
}
