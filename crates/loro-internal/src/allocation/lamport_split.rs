use std::u32;

use crate::{
    dag::{Dag, DagNode},
    id::ID,
};
use fxhash::{FxHashMap, FxHashSet};
use itertools::{min, Itertools};

pub(crate) fn calc_critical_version_lamport_split<T: DagNode, D: Dag<Node = T>>(
    graph: &D,
    start_id_list: &[ID],
    end_id_list: &[ID],
) -> Vec<ID> {
    let mut runner = LamportSplit::new();
    runner.run(graph, start_id_list, end_id_list)
}

struct LamportSplit {
    lamport: FxHashMap<ID, u32>,
    counter: FxHashSet<u32>,
    blacklist: FxHashSet<u32>,
}

impl LamportSplit {
    fn new() -> Self {
        Self {
            lamport: FxHashMap::default(),
            counter: FxHashSet::default(),
            blacklist: FxHashSet::default(),
        }
    }

    fn run<T: DagNode, D: Dag<Node = T>>(
        &mut self,
        graph: &D,
        start_id_list: &[ID],
        end_id_list: &[ID],
    ) -> Vec<ID> {
        println!("--------------------------------------");
        let start_id_set: FxHashSet<ID> = start_id_list.iter().cloned().collect();
        let end_id_set: FxHashSet<ID> = end_id_list.iter().cloned().collect();
        for start in start_id_list {
            self.calc_lamport(graph, start);
        }
        let mut vis: FxHashSet<ID> = FxHashSet::default();
        let mut id = ID {
            peer: 0,
            counter: -1,
        };
        let mut min_lamport: u32 = u32::MAX;
        for start in start_id_list {
            self.search(graph, start, &mut vis, &end_id_set);
            let lamport = self.lamport.get(start).unwrap();
            if min_lamport > *lamport {
                min_lamport = *lamport;
                id = *start;
            }
        }
        let mut minway: Vec<ID> = Vec::new();
        self.minway(graph, &id, &mut minway, &end_id_set);
        for xxx in &minway {
            println!("saass {}", xxx);
        }
        minway
            .iter()
            .skip(1)
            .filter(|x| {
                self.counter.contains(self.lamport.get(*x).unwrap())
                    && !start_id_set.contains(x)
                    && !end_id_set.contains(x)
            })
            .copied()
            .collect_vec()
    }

    fn calc_lamport<T: DagNode, D: Dag<Node = T>>(&mut self, graph: &D, current: &ID) {
        let node = graph.get(*current).unwrap();
        if node.deps().is_empty() {
            self.lamport.insert(*current, 0);
        } else {
            let mut min_lamport: u32 = 0;
            for to_id in node.deps() {
                let to_yes = self.lamport.get(to_id);
                if let Some(x) = to_yes {
                    if min_lamport < *x {
                        min_lamport = *x;
                    }
                } else {
                    self.calc_lamport(graph, to_id);
                    let x = self.lamport.get(to_id).unwrap();
                    if min_lamport < *x {
                        min_lamport = *x;
                    }
                }
            }
            self.lamport.insert(*current, min_lamport + 1);
            println!("{} {}", current, min_lamport + 1)
        }
    }

    fn minway<T: DagNode, D: Dag<Node = T>>(
        &mut self,
        graph: &D,
        current: &ID,
        result: &mut Vec<ID>,
        end_id_set: &FxHashSet<ID>,
    ) {
        result.push(*current);
        let mut id = ID {
            peer: 0,
            counter: -1,
        };
        let mut min_lamport: u32 = u32::MAX;
        for &to_id in graph.get(*current).unwrap().deps() {
            let current_lamport = graph.get(to_id).unwrap().lamport();
            if current_lamport < min_lamport {
                min_lamport = current_lamport;
                id = to_id;
            }
        }
        if id.counter != -1 && end_id_set.contains(&id) {
            self.minway(graph, &id, result, end_id_set);
        }
    }

    fn search<T: DagNode, D: Dag<Node = T>>(
        &mut self,
        graph: &D,
        current: &ID,
        vis: &mut FxHashSet<ID>,
        end_id_set: &FxHashSet<ID>,
    ) {
        vis.insert(*current);
        let lamport = self.lamport.get(current).unwrap();
        if self.counter.contains(lamport) {
            self.blacklist.insert(*lamport);
            self.counter.remove(lamport);
        } else {
            if !self.blacklist.contains(lamport) {
                self.counter.insert(*lamport);
            }
        }
        if !end_id_set.contains(current) {
            for to_id in graph.get(*current).unwrap().deps() {
                if !vis.contains(to_id) {
                    self.search(graph, to_id, vis, end_id_set);
                }
            }
        }
    }
}
