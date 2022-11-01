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
};
use num::traits::AsPrimitive;
use rle::{HasIndex, HasLength, Mergable, RleVec, Sliceable};
use smallvec::SmallVec;

pub type Timestamp = i64;
pub type Lamport = u32;

/// A `Change` contains a list of [Op]s.
#[derive(Debug, Clone)]
pub struct Change<O = Op> {
    pub(crate) ops: RleVec<[O; 2]>,
    pub(crate) deps: SmallVec<[ID; 2]>,
    /// id of the first op in the change
    pub(crate) id: ID,
    /// Lamport timestamp of the change. It can be calculated from deps
    pub(crate) lamport: Lamport,
    /// [Unix time](https://en.wikipedia.org/wiki/Unix_time)
    /// It is the number of seconds that have elapsed since 00:00:00 UTC on 1 January 1970.
    pub(crate) timestamp: Timestamp,
    /// if other changes dep on the middle of this change, we need to record a break point here.
    /// So that we can iter the ops in the correct order.
    ///
    /// If some change deps on counter `t`, then there will be a break point at counter `t`.
    /// - In that case we need to slice the change by counter range of [`start_counter`, `t` + 1)
    ///
    /// TODO: Need tests
    /// Seems like we only need to record it when other changes dep on this change
    pub(crate) break_points: SmallVec<[Counter; 2]>,
}

impl<O> Change<O> {
    pub fn new(
        ops: RleVec<[O; 2]>,
        deps: SmallVec<[ID; 2]>,
        id: ID,
        lamport: Lamport,
        timestamp: Timestamp,
        _freezed: bool,
    ) -> Self {
        Change {
            ops,
            deps,
            id,
            lamport,
            timestamp,
            break_points: SmallVec::new(),
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

impl<O: Mergable + HasLength + HasIndex> HasLength for Change<O> {
    fn content_len(&self) -> usize {
        self.ops.span().as_()
    }
}

#[derive(Debug)]
pub struct ChangeMergeCfg {
    pub max_change_length: usize,
    pub max_change_interval: usize,
    pub from_this_client: bool,
}

impl ChangeMergeCfg {
    pub fn new(from_this: bool) -> Self {
        ChangeMergeCfg {
            from_this_client: from_this,
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
            from_this_client: false,
        }
    }
}

impl Mergable<ChangeMergeCfg> for Change {
    fn merge(&mut self, other: &Self, _: &ChangeMergeCfg) {
        self.ops.merge(&other.ops, &());
    }

    fn is_mergable(&self, other: &Self, cfg: &ChangeMergeCfg) -> bool {
        if !cfg.from_this_client {
            return false;
        }

        if other.deps.is_empty() || !(other.deps.len() == 1 && self.id_last() == other.deps[0]) {
            return false;
        }

        if self.content_len() > cfg.max_change_length {
            return false;
        }

        if other.timestamp - self.timestamp > cfg.max_change_interval as i64 {
            return false;
        }

        self.id.client_id == other.id.client_id
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
                smallvec::smallvec![self.id.inc(from as Counter - 1)]
            } else {
                self.deps.clone()
            },
            id: self.id.inc(from as Counter),
            lamport: self.lamport + from as Lamport,
            timestamp: self.timestamp,
            break_points: self.break_points.clone(),
        }
    }
}

impl DagNode for Change {
    fn deps(&self) -> &[ID] {
        &self.deps
    }
}
