use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::Op;

use crate::id::Counter;
use crate::version::TotalOrderStamp;

use crate::op::OpProxy;

use crate::id::ClientID;

use fxhash::FxHashMap;
use rle::HasLength;

use crate::change::ChangeMergeCfg;

use crate::change::Change;

use rle::RleVec;

// TODO: tests
pub struct OpIter<'a> {
    init: bool,
    heap: BinaryHeap<Reverse<OpProxy<'a>>>,
    changes: &'a FxHashMap<ClientID, RleVec<Change, ChangeMergeCfg>>,
}

impl<'a> OpIter<'a> {
    #[inline]
    pub fn new(changes: &'a FxHashMap<ClientID, RleVec<Change, ChangeMergeCfg>>) -> Self {
        OpIter {
            changes,
            init: false,
            heap: BinaryHeap::new(),
        }
    }
}

impl<'a> Iterator for OpIter<'a> {
    type Item = OpProxy<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        if !self.init {
            self.init = true;
            for changes in self.changes.values() {
                for change in changes.vec().iter() {
                    let mut break_idx = 0;
                    for op in change.ops.vec().iter() {
                        let mut start = 0;
                        while let Some(break_counter) = change.break_points.get(break_idx) {
                            let op_end_counter = op.id.counter + op.len() as Counter - 1;
                            if op_end_counter > *break_counter {
                                let end = break_counter + 1 - op.id.counter;
                                self.heap
                                    .push(Reverse(OpProxy::new(change, op, Some(start..end))));
                                start = end;
                                break_idx += 1;
                            }
                        }

                        self.heap.push(Reverse(OpProxy::new(
                            change,
                            op,
                            Some(start..(op.len() as u32)),
                        )));
                    }
                }
            }
        }

        Some(self.heap.pop()?.0)
    }
}

pub struct ClientOpIter<'a> {
    pub(crate) change_index: usize,
    pub(crate) op_index: usize,
    pub(crate) changes: Option<&'a RleVec<Change, ChangeMergeCfg>>,
}

impl<'a> Iterator for ClientOpIter<'a> {
    type Item = (&'a Change, &'a Op);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(change) = self.changes?.get_merged(self.change_index) {
                if let Some(op) = change.ops.get_merged(self.op_index) {
                    self.op_index += 1;
                    return Some((change, op));
                } else {
                    self.op_index = 0;
                    self.change_index += 1;
                }
            } else {
                return None;
            }
        }
    }
}
