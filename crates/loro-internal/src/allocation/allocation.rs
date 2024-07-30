use itertools::Itertools;

use crate::{
    dag::{Dag, DagNode},
    id::ID,
};

use std::{
    collections::{HashMap, HashSet},
    mem::swap,
    vec,
};

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
    tree: HashMap<ID, ID>,
}

impl AllocationTree {
    fn new() -> Self {
        Self {
            scale: 0,
            topo: vec![],
            deep: DeepOrInd::new(),
            anti_graph: AntiGraph::new(),
            father: Father::new(),
            tree: HashMap::new(),
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
            let end_id_set: HashSet<ID> = end_id_list.iter().cloned().collect();
            let mut vis: HashSet<ID> = HashSet::new();
            for &to in start_id_list {
                self.calc_ind::<T, D>(graph, to, &end_id_set, &mut vis, &mut ind);
            }
            self.scale = ((vis.len() as f64).log2() as usize) + 1;
            for id in vis {
                self.anti_graph.init(&id);
                self.deep.init(&id);
                self.father.init(&id, &self.scale);
            }
            self.topo_sort(graph, start_id_list, &end_id_set, &mut ind);
        }
        self.calc::<T, D>();
        let start_id_set = start_id_list.iter().cloned().collect();
        self.resolve(&end_id_list, &start_id_set)
    }

    fn calc_ind<T: DagNode, D: Dag<Node = T>>(
        &mut self,
        graph: &D,
        start_id: ID,
        end_id_set: &HashSet<ID>,
        vis: &mut HashSet<ID>,
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
        end_id_set: &HashSet<ID>,
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
    }

    fn lca(&self, mut u: ID, mut v: ID) -> ID {
        if self.deep.get(&u) < self.deep.get(&v) {
            swap(&mut u, &mut v);
        }
        for i in (0..=self.scale).rev() {
            if self.deep.get(&self.father.get(&u, i)) >= self.deep.get(&v) {
                u = self.father.get(&u, i);
            }
        }
        if u == v {
            return u;
        }
        for i in (0..=self.scale).rev() {
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
                let mut v = ve[0];
                for i in 1..ve.len() {
                    v = self.lca(v, ve[i]);
                }
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

    fn resolve(&self, end_id_list: &[ID], start_id_set: &HashSet<ID>) -> Vec<ID> {
        let mut result: HashSet<ID> = HashSet::new();
        for &u in end_id_list {
            let mut ux = u;
            result.insert(ux);
            while !start_id_set.contains(&ux) {
                let father = self.tree.get(&ux).unwrap();
                result.insert(*father);
                ux = *father;
            }
        }
        result.iter().cloned().collect_vec()
    }
}
