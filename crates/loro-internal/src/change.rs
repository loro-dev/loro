//! [Change]s are merged ops.
//!
//! Every [Change] has deps on other [Change]s. All [Change]s in the document thus form a DAG.
//!
//! Note: `dep` can only point to the end of the other [Change]. This is the invariant of [Change]s.

use crate::{
    dag::DagNode,
    estimated_size::EstimatedSize,
    id::{Counter, ID},
    op::Op,
    span::{HasId, HasLamport},
    version::Frontiers,
};
use loro_common::{HasCounter, HasCounterSpan, PeerID};
use num::traits::AsPrimitive;
use rle::{HasIndex, HasLength, Mergable, RleVec, Sliceable};
use smallvec::SmallVec;

pub type Timestamp = i64;
pub type Lamport = u32;

/// A `Change` contains a list of [Op]s.
///
/// When undo/redo we should always undo/redo a whole [Change].
// PERF change slice and getting length is kinda slow I guess
#[derive(Debug, Clone, PartialEq)]
pub struct Change<O = Op> {
    /// id of the first op in the change
    pub(crate) id: ID,
    /// Lamport timestamp of the change. It can be calculated from deps
    pub(crate) lamport: Lamport,
    pub(crate) deps: Frontiers,
    /// [Unix time](https://en.wikipedia.org/wiki/Unix_time)
    /// It is the number of seconds that have elapsed since 00:00:00 UTC on 1 January 1970.
    pub(crate) timestamp: Timestamp,
    pub(crate) commit_msg: Option<Arc<str>>,
    pub(crate) ops: RleVec<[O; 1]>,
}

pub(crate) struct ChangeRef<'a, O = Op> {
    pub(crate) id: &'a ID,
    pub(crate) lamport: &'a Lamport,
    pub(crate) deps: &'a Frontiers,
    pub(crate) timestamp: &'a Timestamp,
    pub(crate) commit_msg: &'a Option<Arc<str>>,
    pub(crate) ops: &'a RleVec<[O; 1]>,
}

impl<'a, O> ChangeRef<'a, O> {
    pub fn from_change(change: &'a Change<O>) -> Self {
        Self {
            id: &change.id,
            lamport: &change.lamport,
            deps: &change.deps,
            timestamp: &change.timestamp,
            commit_msg: &change.commit_msg,
            ops: &change.ops,
        }
    }
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
            commit_msg: None,
        }
    }

    #[inline]
    pub fn ops(&self) -> &RleVec<[O; 1]> {
        &self.ops
    }

    #[inline]
    pub fn deps(&self) -> &Frontiers {
        &self.deps
    }

    #[inline]
    pub fn peer(&self) -> PeerID {
        self.id.peer
    }

    #[inline]
    pub fn lamport(&self) -> Lamport {
        self.lamport
    }

    #[inline]
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    #[inline]
    pub fn id(&self) -> ID {
        self.id
    }

    #[inline]
    pub fn deps_on_self(&self) -> bool {
        if let Some(id) = self.deps.as_single() {
            id.peer == self.id.peer
        } else {
            false
        }
    }

    pub fn message(&self) -> Option<&Arc<str>> {
        self.commit_msg.as_ref()
    }
}

impl<O: EstimatedSize> EstimatedSize for Change<O> {
    /// Estimate the storage size of the change in bytes
    #[inline]
    fn estimate_storage_size(&self) -> usize {
        let id_size = 2;
        let lamport_size = 1;
        let timestamp_size = 1;
        let deps_size = (self.deps.len().max(1) - 1) * 4;
        let ops_size = self
            .ops
            .iter()
            .map(|op| op.estimate_storage_size())
            .sum::<usize>();
        id_size + lamport_size + timestamp_size + ops_size + deps_size
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

impl<O> HasCounter for Change<O> {
    fn ctr_start(&self) -> Counter {
        self.id.counter
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

impl<O: Mergable + HasLength + HasIndex + Debug> Change<O> {
    pub fn len(&self) -> usize {
        self.ops.span().as_()
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

use std::{fmt::Debug, sync::Arc};
impl<O: Mergable + HasLength + HasIndex + Debug> HasLength for Change<O> {
    fn content_len(&self) -> usize {
        self.ops.span().as_()
    }
}

impl<O: Mergable + HasLength + HasIndex + Sliceable + HasCounter + Debug> Sliceable for Change<O> {
    // TODO: feels slow, need to confirm whether this affects performance
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(from < to);
        assert!(to <= self.atom_len());
        let from_counter = self.id.counter + from as Counter;
        let to_counter = self.id.counter + to as Counter;
        let ops = {
            if from >= to {
                RleVec::new()
            } else {
                let mut ans: SmallVec<[_; 1]> = SmallVec::new();
                let mut start_index = 0;
                if self.ops.len() >= 8 {
                    let result = self
                        .ops
                        .binary_search_by(|op| op.ctr_end().cmp(&from_counter));
                    start_index = match result {
                        Ok(i) => i,
                        Err(i) => i,
                    };
                }

                for i in start_index..self.ops.len() {
                    let op = &self.ops[i];
                    if op.ctr_start() >= to_counter {
                        break;
                    }
                    if op.ctr_end() <= from_counter {
                        continue;
                    }

                    let start_offset =
                        ((from_counter - op.ctr_start()).max(0) as usize).min(op.atom_len());
                    let end_offset =
                        ((to_counter - op.ctr_start()).max(0) as usize).min(op.atom_len());
                    assert_ne!(start_offset, end_offset);
                    ans.push(op.slice(start_offset, end_offset))
                }

                RleVec::from(ans)
            }
        };
        assert_eq!(ops.first().unwrap().ctr_start(), from_counter);
        assert_eq!(ops.last().unwrap().ctr_end(), to_counter);
        Self {
            ops,
            deps: if from > 0 {
                Frontiers::from_id(self.id.inc(from as Counter - 1))
            } else {
                self.deps.clone()
            },
            id: self.id.inc(from as Counter),
            lamport: self.lamport + from as Lamport,
            timestamp: self.timestamp,
            commit_msg: self.commit_msg.clone(),
        }
    }
}

impl DagNode for Change {
    fn deps(&self) -> &Frontiers {
        &self.deps
    }
}

impl Change {
    pub fn can_merge_right(&self, other: &Self, merge_interval: i64) -> bool {
        if other.id.peer == self.id.peer
            && other.id.counter == self.id.counter + self.content_len() as Counter
            && other.deps.len() == 1
            && other.deps.as_single().unwrap().peer == self.id.peer
            && other.timestamp - self.timestamp <= merge_interval
            && self.commit_msg == other.commit_msg
        {
            debug_assert!(other.timestamp >= self.timestamp);
            debug_assert!(other.lamport == self.lamport + self.len() as Lamport);
            true
        } else {
            false
        }
    }
}

/// [Unix time](https://en.wikipedia.org/wiki/Unix_time)
/// It is the number of milliseconds that have elapsed since 00:00:00 UTC on 1 January 1970.
#[cfg(not(all(feature = "wasm", target_arch = "wasm32")))]
pub(crate) fn get_sys_timestamp() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .as_()
}

/// [Unix time](https://en.wikipedia.org/wiki/Unix_time)
/// It is the number of seconds that have elapsed since 00:00:00 UTC on 1 January 1970.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub fn get_sys_timestamp() -> f64 {
    use wasm_bindgen::prelude::wasm_bindgen;
    #[wasm_bindgen]
    extern "C" {
        // Use `js_namespace` here to bind `console.log(..)` instead of just
        // `log(..)`
        #[wasm_bindgen(js_namespace = Date)]
        pub fn now() -> f64;
    }

    now()
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn size_of_change() {
        let size = std::mem::size_of::<Change>();
        println!("{}", size);
    }
}
