use std::{
    collections::{BinaryHeap, VecDeque},
    sync::Arc,
};

use loro_common::{CounterSpan, HasIdSpan, HasLamport};
use rle::RleCollection;

use crate::{change::Change, OpLog, VersionVector};

use super::{change_store::ChangesBlock, AppDag, AppDagNode, BlockChangeRef};

pub(crate) struct PeerChangesIter {
    blocks: VecDeque<Arc<ChangesBlock>>,
    change_index: usize,
    counter_range: CounterSpan,
}

impl PeerChangesIter {
    fn new_change_iter_rev(mut changes: VecDeque<Arc<ChangesBlock>>, counter: CounterSpan) -> Self {
        let mut index = changes
            .back()
            .map(|x| x.content().len_changes().saturating_sub(1))
            .unwrap_or(0);

        while let Some(block) = changes.back() {
            if let Some(change) = block.content().try_changes().unwrap().get(index) {
                if change.id.counter < counter.end {
                    break;
                }
            } else if index == 0 {
                changes.pop_back();
            } else {
                index -= 1;
            }
        }

        PeerChangesIter {
            blocks: changes,
            change_index: index,
            counter_range: counter,
        }
    }

    fn current_weight(&self) -> i32 {
        if self.blocks.is_empty() {
            return 0;
        }

        self.blocks
            .back()
            .map(|x| {
                x.content()
                    .try_changes()
                    .unwrap()
                    .get(self.change_index)
                    .unwrap()
                    .lamport as i32
            })
            .unwrap_or(0)
    }
}

impl Ord for PeerChangesIter {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.current_weight().cmp(&other.current_weight())
    }
}

impl PartialOrd for PeerChangesIter {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for PeerChangesIter {
    fn eq(&self, other: &Self) -> bool {
        self.current_weight() == other.current_weight()
    }
}

impl Eq for PeerChangesIter {}

impl Iterator for PeerChangesIter {
    type Item = BlockChangeRef;

    fn next(&mut self) -> Option<Self::Item> {
        if self.blocks.is_empty() {
            return None;
        }

        let c = BlockChangeRef {
            block: self.blocks.back().unwrap().clone(),
            change_index: self.change_index,
        };

        if c.id_last().counter < self.counter_range.start {
            return None;
        }

        let ans = Some(c);
        if self.change_index == 0 {
            self.blocks.pop_back();
        } else {
            self.change_index -= 1;
        }

        ans
    }
}

pub(crate) struct MergedChangeIter {
    heap: BinaryHeap<PeerChangesIter>,
}

impl Iterator for MergedChangeIter {
    type Item = BlockChangeRef;

    fn next(&mut self) -> Option<Self::Item> {
        let mut iter = self.heap.pop()?;
        let ans = iter.next();
        self.heap.push(iter);
        ans
    }
}

impl MergedChangeIter {
    pub fn new_change_iter_rev(oplog: &OpLog, from: &VersionVector, to: &VersionVector) -> Self {
        let mut heap = BinaryHeap::new();
        for span in to.sub_iter(from) {
            let blocks = oplog.change_store.get_blocks_in_range(span);
            let iter = PeerChangesIter::new_change_iter_rev(blocks, span.counter);
            heap.push(iter);
        }

        Self { heap }
    }
}
