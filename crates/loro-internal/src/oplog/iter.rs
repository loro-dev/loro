use std::collections::BinaryHeap;

use loro_common::{CounterSpan, HasLamport};
use rle::RleCollection;

use crate::{change::Change, OpLog, VersionVector};

use super::{AppDag, AppDagNode};

pub(crate) struct PeerChangesIter<'a, T> {
    changes: &'a [T],
    is_forward: bool,
}

impl<T: HasLamport> PeerChangesIter<'_, T> {
    fn current_weight(&self) -> i32 {
        if self.changes.is_empty() {
            return 0;
        }

        if self.is_forward {
            // Need to be reversed so that the top element in the max heap
            // has the smallest lamport
            -(self.changes.first().unwrap().lamport() as i32)
        } else {
            self.changes.last().unwrap().lamport() as i32
        }
    }
}

impl<T: HasLamport> Ord for PeerChangesIter<'_, T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.current_weight().cmp(&other.current_weight())
    }
}

impl<T: HasLamport> PartialOrd for PeerChangesIter<'_, T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: HasLamport> PartialEq for PeerChangesIter<'_, T> {
    fn eq(&self, other: &Self) -> bool {
        self.current_weight() == other.current_weight()
    }
}

impl<T: HasLamport> Eq for PeerChangesIter<'_, T> {}

impl<'a, T: HasLamport> Iterator for PeerChangesIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.changes.is_empty() {
            return None;
        }

        if self.is_forward {
            let (next, rest) = self.changes.split_first().unwrap();
            self.changes = rest;
            Some(next)
        } else {
            let (next, rest) = self.changes.split_last().unwrap();
            self.changes = rest;
            Some(next)
        }
    }
}

pub(crate) struct MergedChangeIter<'a, T> {
    heap: BinaryHeap<PeerChangesIter<'a, T>>,
}

impl<'a, T: HasLamport> Iterator for MergedChangeIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let mut iter = self.heap.pop()?;
        let ans = iter.next();
        self.heap.push(iter);
        ans
    }
}

#[allow(unused)]
impl PeerChangesIter<'_, AppDagNode> {
    pub(crate) fn new_dag_iter(
        changes: &Vec<AppDagNode>,
        counter: CounterSpan,
        is_forward: bool,
    ) -> PeerChangesIter<'_, AppDagNode> {
        assert!(counter.start < counter.end);
        let start = changes
            .get_by_atom_index(counter.start)
            .unwrap()
            .merged_index;
        let end = changes
            .get_by_atom_index(counter.end - 1)
            .unwrap()
            .merged_index;
        let changes = &changes[start..=end];
        PeerChangesIter {
            changes,
            is_forward,
        }
    }
}

#[allow(unused)]
impl<'a> MergedChangeIter<'a, AppDagNode> {
    pub fn new_dag_iter(
        dag: &'a AppDag,
        from: &VersionVector,
        to: &VersionVector,
        is_forward: bool,
    ) -> Self {
        let mut heap = BinaryHeap::new();
        for span in to.sub_iter(from) {
            let nodes = dag.map.get(&span.peer).unwrap();
            let iter = PeerChangesIter::new_dag_iter(nodes, span.counter, is_forward);
            heap.push(iter);
        }

        Self { heap }
    }
}

impl PeerChangesIter<'_, Change> {
    pub(crate) fn new_change_iter(
        changes: &Vec<Change>,
        counter: CounterSpan,
        is_forward: bool,
    ) -> PeerChangesIter<'_, Change> {
        assert!(counter.start < counter.end);
        let start = changes
            .get_by_atom_index(counter.start)
            .unwrap()
            .merged_index;
        let end = changes
            .get_by_atom_index(counter.end - 1)
            .unwrap()
            .merged_index;
        let changes = &changes[start..=end];
        PeerChangesIter {
            changes,
            is_forward,
        }
    }
}

impl<'a> MergedChangeIter<'a, Change> {
    pub fn new_change_iter(
        oplog: &'a OpLog,
        from: &VersionVector,
        to: &VersionVector,
        is_forward: bool,
    ) -> Self {
        let mut heap = BinaryHeap::new();
        for span in to.sub_iter(from) {
            let nodes = oplog.changes.get(&span.peer).unwrap();
            let iter = PeerChangesIter::new_change_iter(nodes, span.counter, is_forward);
            heap.push(iter);
        }

        Self { heap }
    }
}
