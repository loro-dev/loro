use crate::{
    dag::{Dag, DagNode},
    id::ID,
};

use fxhash::{FxHashMap, FxHashSet};
use std::collections::BinaryHeap;

#[derive(Debug, PartialEq, Eq)]
struct SortBase {
    id: ID,
    lamport: u32,
}

impl PartialOrd for SortBase {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.lamport.partial_cmp(&other.lamport)
    }
}

impl Ord for SortBase {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.lamport.cmp(&other.lamport)
    }
}

pub struct BfsBody {
    queue: BinaryHeap<SortBase>,
    lamport: FxHashMap<ID, u32>,
}

pub fn calc_critical_version_bfs<T: DagNode, D: Dag<Node = T>>(
    graph: &D,
    start_list: &[ID],
) -> Vec<ID> {
    let mut runner = BfsBody::new();
    runner.run(graph, start_list)
}

impl BfsBody {
    fn new() -> Self {
        Self {
            queue: BinaryHeap::new(),
            lamport: FxHashMap::default(),
        }
    }

    fn run<T: DagNode, D: Dag<Node = T>>(&mut self, graph: &D, start_list: &[ID]) -> Vec<ID> {
        let mut start_end_set: FxHashSet<ID> = start_list.iter().cloned().collect();
        for start in start_list {
            self.calc_lamport(graph, start);
            self.queue.push(SortBase {
                id: *start,
                lamport: *self.lamport.get(start).unwrap(),
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
                for to_id in node.deps() {
                    self.queue.push(SortBase {
                        id: *to_id,
                        lamport: *self.lamport.get(to_id).unwrap(),
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

    fn calc_lamport<T: DagNode, D: Dag<Node = T>>(&mut self, graph: &D, current: &ID) {
        let node = graph.get(*current).unwrap();
        if node.deps().is_empty() {
            self.lamport.insert(*current, 0);
        } else {
            let mut max_lamport: u32 = 0;
            for to_id in node.deps() {
                let to_yes = self.lamport.get(to_id);
                if let Some(x) = to_yes {
                    if max_lamport < *x {
                        max_lamport = *x;
                    }
                } else {
                    self.calc_lamport(graph, to_id);
                    let x = self.lamport.get(to_id).unwrap();
                    if max_lamport < *x {
                        max_lamport = *x;
                    }
                }
            }
            self.lamport.insert(*current, max_lamport + 1);
        }
    }
}
