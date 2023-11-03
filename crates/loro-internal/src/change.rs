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
use loro_common::{HasCounter, HasCounterSpan};
use num::traits::AsPrimitive;
use rle::{HasIndex, HasLength, Mergable, RleVec, Sliceable};
use smallvec::SmallVec;

pub type Timestamp = i64;
pub type Lamport = u32;

/// A `Change` contains a list of [Op]s.
///
/// When undo/redo we should always undo/redo a whole [Change].
// PERF change slice and getting length is kinda slow I guess
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
    /// if it has dependents, it cannot merge with new changes
    pub(crate) has_dependents: bool,
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
            has_dependents: false,
        }
    }

    pub fn ops(&self) -> &RleVec<[O; 1]> {
        &self.ops
    }

    pub fn lamport(&self) -> Lamport {
        self.lamport
    }

    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    pub fn id(&self) -> ID {
        self.id
    }

    pub fn deps_on_self(&self) -> bool {
        self.deps.len() == 1 && self.deps[0].peer == self.id.peer
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
            has_dependents: self.has_dependents,
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

#[cfg(not(all(feature = "wasm", target_arch = "wasm32")))]
pub(crate) fn get_sys_timestamp() -> Timestamp {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .as_()
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub fn get_sys_timestamp() -> Timestamp {
    use wasm_bindgen::prelude::wasm_bindgen;
    #[wasm_bindgen]
    extern "C" {
        // Use `js_namespace` here to bind `console.log(..)` instead of just
        // `log(..)`
        #[wasm_bindgen(js_namespace = Date)]
        pub fn now() -> f64;
    }

    now() as Timestamp
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
