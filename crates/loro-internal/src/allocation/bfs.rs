#![allow(dead_code)]
use crate::{
    dag::{Dag, DagNode},
    id::ID,
    version::Frontiers,
};

use fxhash::FxHashSet;
use std::collections::BinaryHeap;

#[derive(Debug, PartialEq, Eq)]
struct SortBase {
    id: ID,
    lamport: u32,
}

impl PartialOrd for SortBase {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SortBase {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.lamport.cmp(&other.lamport)
    }
}

pub struct BfsBody {
    queue: BinaryHeap<SortBase>,
    visited: FxHashSet<ID>,
}

pub fn calc_critical_version_bfs<T: DagNode, D: Dag<Node = T>>(
    graph: &D,
    start_list: &Frontiers,
) -> Vec<ID> {
    let mut runner = BfsBody::new();
    runner.run(graph, start_list)
}

impl BfsBody {
    fn new() -> Self {
        Self {
            queue: BinaryHeap::new(),
            visited: FxHashSet::default(),
        }
    }

    fn run<T: DagNode, D: Dag<Node = T>>(&mut self, graph: &D, start_list: &Frontiers) -> Vec<ID> {
        let mut start_end_set: FxHashSet<ID> = start_list.iter().collect();
        for start in start_list.iter() {
            self.queue.push(SortBase {
                id: start,
                lamport: graph.get(start).unwrap().lamport(),
            });
        }
        let mut result: Vec<ID> = Vec::new();
        while let Some(SortBase { id, lamport: _ }) = self.queue.pop() {
            if self.queue.is_empty() {
                result.push(id);
            }
            let node = graph.get(id).unwrap();
            if node.deps().is_empty() {
                start_end_set.insert(id);
            } else {
                for to_id in node.deps().iter() {
                    if self.visited.contains(&to_id) {
                        continue;
                    }
                    self.visited.insert(to_id);
                    self.queue.push(SortBase {
                        id: to_id,
                        lamport: graph.get(to_id).unwrap().lamport(),
                    });
                }
            }
        }
        result
            .iter()
            .filter(|id| !start_end_set.contains(id))
            .cloned()
            .collect()
    }
}
