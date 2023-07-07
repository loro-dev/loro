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
    span::{HasId, HasLamport},
    version::Frontiers,
};
use num::traits::AsPrimitive;
use rle::{HasIndex, HasLength, Mergable, RleVec, Sliceable};

pub type Timestamp = i64;
pub type Lamport = u32;

/// A `Change` contains a list of [Op]s.
///
/// When undo/redo we should always undo/redo a whole [Change].
#[derive(Debug, Clone)]
pub struct Change<O = Op> {
    pub(crate) ops: RleVec<[O; 1]>,
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
        ops: RleVec<[O; 1]>,
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

impl<O: Mergable + HasLength + HasIndex + Debug> HasIndex for Change<O> {
    type Int = Counter;

    fn get_start_index(&self) -> Self::Int {
        self.id.counter
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

impl<O> Mergable for Change<O> {
    fn is_mergable(&self, _other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        false
    }

    fn merge(&mut self, _other: &Self, _conf: &())
    where
        Self: Sized,
    {
        unreachable!()
    }
}

use std::fmt::Debug;
impl<O: Mergable + HasLength + HasIndex + Debug> HasLength for Change<O> {
    fn content_len(&self) -> usize {
        self.ops.span().as_()
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

impl Change {
    pub fn can_merge_right(&self, other: &Self) -> bool {
        other.id.peer == self.id.peer
            && other.id.counter == self.id.counter + self.content_len() as Counter
            && other.deps.len() == 1
            && other.deps[0].peer == self.id.peer
    }
}
