use std::u32;

use crate::{
    dag::{Dag, DagNode},
    id::ID,
};
use fxhash::FxHashSet;
use itertools::Itertools;

pub(crate) fn calc_critical_version_lamport_split<T: DagNode, D: Dag<Node = T>>(
    graph: &D,
    start_id_list: &[ID],
    end_id_list: &[ID],
) -> Vec<ID> {
    let mut runner = LamportSplit::new();
    runner.run(graph, start_id_list, end_id_list)
}

struct LamportSplit {
    counter: FxHashSet<u32>,
    blacklist: FxHashSet<u32>,
}

impl LamportSplit {
    fn new() -> Self {
        Self {
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
        let end_id_set: FxHashSet<ID> = end_id_list.iter().cloned().collect();
        let mut vis: FxHashSet<ID> = FxHashSet::default();
        for start in start_id_list {
            self.search(graph, start, &mut vis, &end_id_set);
        }
        let mut minway: Vec<ID> = Vec::new();
        self.minway(graph, &start_id_list[0], &mut minway, &end_id_set);
        for a in &vis {
            println!("bbb {} {}", a, graph.get(*a).unwrap().lamport());
        }
        minway
            .iter()
            .filter(|x| self.counter.contains(&graph.get(**x).unwrap().lamport()))
            .copied()
            .collect_vec()
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
        if id.counter != -1 && !end_id_set.contains(current) {
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
        let node = graph.get(*current).unwrap();
        let lamport = node.lamport();
        if self.counter.contains(&lamport) {
            self.blacklist.insert(lamport);
            self.counter.remove(&lamport);
            println!(
                "{} {} {}",
                current,
                lamport,
                self.counter.contains(&lamport)
            );
        } else {
            if !self.blacklist.contains(&lamport) {
                self.counter.insert(lamport);
            }
        }
        if !end_id_set.contains(current) {
            for to_id in node.deps() {
                if !vis.contains(to_id) {
                    self.search(graph, to_id, vis, end_id_set);
                }
            }
        }
    }
}
