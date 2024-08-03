use fxhash::{FxHashMap, FxHashSet};
use itertools::Itertools;

use crate::{
    dag::{Dag, DagNode},
    id::ID,
};

use std::{mem::swap, vec};

use super::types::{AntiGraph, DeepOrInd, Father};

pub(crate) fn calc_critical_version<T: DagNode, D: Dag<Node = T>>(
    graph: &D,
    start_id_list: &[ID],
    end_id_list: &[ID],
) -> Vec<ID> {
    let mut alloc = AllocationTree::new();
    alloc.run::<T, D>(graph, start_id_list, end_id_list)
}

struct AllocationTree {
    scale: usize,
    topo: Vec<ID>,
    deep: DeepOrInd,
    anti_graph: AntiGraph,
    father: Father,
    tree: FxHashMap<ID, ID>,
    virtual_end_point: ID,
    virtual_start_point: ID,
}

fn log2_floor(mut x: usize) -> usize {
    x |= x >> 1;
    x |= x >> 2;
    x |= x >> 4;
    x |= x >> 8;
    x |= x >> 16;
    (x.count_ones() - 1) as usize
}

impl AllocationTree {
    fn new() -> Self {
        Self {
            scale: 0,
            topo: vec![ID {
                peer: 0,
                counter: -1,
            }],
            deep: DeepOrInd::new(),
            anti_graph: AntiGraph::new(),
            father: Father::new(),
            tree: FxHashMap::default(),
            virtual_start_point: ID {
                peer: 0,
                counter: -1,
            },
            virtual_end_point: ID {
                peer: 0,
                counter: -2,
            },
        }
    }

    fn run<T: DagNode, D: Dag<Node = T>>(
        &mut self,
        graph: &D,
        start_id_list: &[ID],
        end_id_list: &[ID],
    ) -> Vec<ID> {
        {
            let mut ind = DeepOrInd::new();
            let end_id_set: FxHashSet<ID> = end_id_list.iter().cloned().collect();
            let mut vis: FxHashSet<ID> = FxHashSet::default();
            for &to in start_id_list {
                self.calc_ind::<T, D>(graph, to, &end_id_set, &mut vis, &mut ind);
            }
            self.scale = log2_floor(vis.len() + 2) + 1;
            vis.insert(self.virtual_start_point);
            vis.insert(self.virtual_end_point);
            for id in vis {
                self.anti_graph.init(&id);
                self.deep.init(&id);
                self.father.init(&id, &self.scale);
            }
            self.topo_sort(graph, start_id_list, &end_id_set, &mut ind);
        }
        self.calc::<T, D>();
        self.resolve()
    }

    fn calc_ind<T: DagNode, D: Dag<Node = T>>(
        &mut self,
        graph: &D,
        start_id: ID,
        end_id_set: &FxHashSet<ID>,
        vis: &mut FxHashSet<ID>,
        ind: &mut DeepOrInd,
    ) {
        vis.insert(start_id);
        if !end_id_set.contains(&start_id) {
            for &to_id in graph.get(start_id).unwrap().deps() {
                self.anti_graph.add(&to_id, &start_id);
                ind.inc(&to_id);
                if vis.contains(&to_id) == false {
                    self.calc_ind(graph, to_id, end_id_set, vis, ind);
                }
            }
        }
    }

    fn topo_sort<T: DagNode, D: Dag<Node = T>>(
        &mut self,
        graph: &D,
        start_id_list: &[ID],
        end_id_set: &FxHashSet<ID>,
        ind: &mut DeepOrInd,
    ) {
        let mut stack: Vec<ID> = start_id_list.iter().cloned().collect_vec();
        while !stack.is_empty() {
            let u = stack.pop().unwrap();
            self.topo.push(u);
            if !end_id_set.contains(&u) {
                for &to_id in graph.get(u).unwrap().deps() {
                    if ind.dec(&to_id) == 0 {
                        stack.push(to_id);
                    }
                }
            }
        }
        self.topo.push(self.virtual_end_point);
        for end in end_id_set {
            self.anti_graph.add(&self.virtual_end_point, end);
        }
        for start in start_id_list {
            self.anti_graph.add(start, &self.virtual_start_point);
        }
    }

    fn lca(&self, mut u: ID, mut v: ID) -> ID {
        if self.deep.get(&u) < self.deep.get(&v) {
            swap(&mut u, &mut v);
        }
        let mut depx = self.deep.get(&u);
        let depy = self.deep.get(&v);
        while depx > depy {
            u = self.father.get(&u, log2_floor(depx - depy));
            depx = self.deep.get(&u);
        }
        if u == v {
            return u;
        }
        for i in (0..=log2_floor(depx)).rev() {
            let tu = self.father.get(&u, i);
            let tv = self.father.get(&v, i);
            if tu != tv {
                u = tu;
                v = tv;
            }
        }
        self.father.get(&u, 0)
    }

    fn calc<T: DagNode, D: Dag<Node = T>>(&mut self) {
        let topo_len = self.topo.len();
        for j in 0..topo_len {
            let u = self.topo[j];
            let ve = self.anti_graph.get(&u);
            if ve.len() != 0 {
                let v = ve
                    .iter()
                    .copied()
                    .reduce(|acc, x| self.lca(acc, x))
                    .unwrap();
                self.tree.insert(u, v);
                let v_add_one = self.deep.get(&v) + 1;
                self.deep.set(&u, v_add_one);
                self.father.set(&u, 0, v);
                for i in 1..=self.scale {
                    let idx = &self.father.get(&u, i - 1);
                    let id = self.father.get(idx, i - 1);
                    self.father.set(&u, i, id);
                }
            }
        }
    }

    fn resolve(&self) -> Vec<ID> {
        let mut result: Vec<ID> = vec![];
        let mut u = self.tree[&self.virtual_end_point];
        while u != self.virtual_start_point {
            result.push(u);
            u = self.tree[&u];
        }
        result
    }
}

#[test]
fn test_fast_log() {
    let test_cases = [
        (1, 0),
        (2, 1),
        (3, 1),
        (4, 2),
        (7, 2),
        (8, 3),
        (15, 3),
        (16, 4),
        (31, 4),
        (32, 5),
        (1023, 9),
        (1024, 10),
        (1025, 10),
    ];
    for (input, expected) in test_cases {
        assert_eq!(
            log2_floor(input),
            expected,
            "fast_floor_log({}) should be {}",
            input,
            expected
        );
    }
}
