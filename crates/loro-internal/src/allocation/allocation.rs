use itertools::Itertools;

use crate::{
    dag::{Dag, DagNode},
    id::ID,
};

use std::{
    collections::{HashMap, HashSet},
    f64::consts::E,
    mem::swap,
    vec,
};

pub(crate) fn calc_critical_version<T: DagNode, D: Dag<Node = T>>(
    graph: &D,
    start_id_list: &[ID],
    end_id_list: &[ID],
) -> Vec<ID> {
    let mut alloc = AllocationTree::new();
    alloc.run::<T, D>(graph, start_id_list, end_id_list)
}

fn add_edge_for_anti_graph(anti_graph: &mut HashMap<ID, Vec<ID>>, from_id: &ID, to_id: &ID) {
    if let Some(x) = anti_graph.get_mut(from_id) {
        x.push(to_id.clone());
    } else {
        anti_graph.insert(from_id.clone(), vec![to_id.clone()]);
    }
}

fn get_edge_in_anti_graph<'a>(anti_graph: &'a HashMap<ID, Vec<ID>>, from_id: &ID) -> &'a Vec<ID> {
    anti_graph.get(from_id).unwrap()
}

fn inc_dep_or_ind(dep_or_ind: &mut HashMap<ID, usize>, id: &ID) {
    if let Some(x) = dep_or_ind.get_mut(id) {
        *x += 1;
    } else {
        dep_or_ind.insert(id.clone(), 1);
    }
}

fn modify_dep(dep: &mut HashMap<ID, usize>, id: &ID, val: usize) {
    let x = dep.get_mut(id).unwrap();
    *x = val;
}

fn dec_ind(ind: &mut HashMap<ID, usize>, id: &ID) -> usize {
    let x = ind.get_mut(id).unwrap();
    *x -= 1;
    *x
}

fn get_dep_or_ind(dep_or_ind: &HashMap<ID, usize>, id: &ID) -> usize {
    if let Some(x) = dep_or_ind.get(id) {
        *x
    } else {
        0
    }
}

fn get_father(father: &HashMap<ID, Vec<ID>>, id: &ID, layer: usize) -> ID {
    if let Some(x) = father.get(id) {
        x[layer]
    } else {
        ID {
            peer: 0,
            counter: 0,
        }
    }
}

fn set_father(father: &mut HashMap<ID, Vec<ID>>, id: &ID, layer: usize, value: ID) {
    let x = father.get_mut(id).unwrap();
    x[layer] = value;
}

struct AllocationTree {
    scale: usize,
    ind: HashMap<ID, usize>,
    topo: Vec<ID>,
    deep: HashMap<ID, usize>,
    anti_graph: HashMap<ID, Vec<ID>>,
    father: HashMap<ID, Vec<ID>>,
    tree: HashMap<ID, ID>,
}

impl AllocationTree {
    fn new() -> Self {
        Self {
            scale: 0,
            ind: HashMap::new(),
            topo: vec![],
            deep: HashMap::new(),
            anti_graph: HashMap::new(),
            father: HashMap::new(),
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
            let end_id_set: HashSet<ID> = end_id_list.iter().cloned().collect();
            let mut vis: HashSet<ID> = HashSet::new();
            for &to in start_id_list {
                self.calc_ind::<T, D>(graph, to, &end_id_set, &mut vis);
            }
            self.scale = ((vis.len() as f64).log2() as usize) + 1;
            for id in vis {
                if !self.anti_graph.contains_key(&id) {
                    self.anti_graph.insert(id, vec![]);
                }
                self.deep.insert(id, 0);
                self.father.insert(
                    id,
                    vec![
                        ID {
                            peer: 0,
                            counter: 0
                        };
                        self.scale + 1
                    ],
                );
            }
            self.topo_sort(graph, start_id_list, &end_id_set);
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
    ) {
        vis.insert(start_id);
        if !end_id_set.contains(&start_id) {
            for &to_id in graph.get(start_id).unwrap().deps() {
                add_edge_for_anti_graph(&mut self.anti_graph, &to_id, &start_id);
                inc_dep_or_ind(&mut self.ind, &to_id);
                if vis.contains(&to_id) == false {
                    self.calc_ind(graph, to_id, end_id_set, vis);
                }
            }
        }
    }
    fn topo_sort<T: DagNode, D: Dag<Node = T>>(
        &mut self,
        graph: &D,
        start_id_list: &[ID],
        end_id_set: &HashSet<ID>,
    ) {
        let mut stack: Vec<ID> = start_id_list.iter().cloned().collect_vec();
        while !stack.is_empty() {
            let u = stack.pop().unwrap();
            self.topo.push(u);
            if !end_id_set.contains(&u) {
                for &to_id in graph.get(u.clone()).unwrap().deps() {
                    if dec_ind(&mut self.ind, &to_id) == 0 {
                        stack.push(to_id);
                    }
                }
            }
        }
    }
    fn lca(&self, mut u: ID, mut v: ID) -> ID {
        if get_dep_or_ind(&self.deep, &u) < get_dep_or_ind(&self.deep, &v) {
            swap(&mut u, &mut v);
        }
        for i in (0..=self.scale).rev() {
            if get_dep_or_ind(&self.deep, &get_father(&self.father, &u, i))
                >= get_dep_or_ind(&self.deep, &v)
            {
                u = get_father(&self.father, &u, i);
            }
        }
        if u == v {
            return u;
        }
        for i in (0..=self.scale).rev() {
            let tu = get_father(&self.father, &u, i);
            let tv = get_father(&self.father, &v, i);
            if tu != tv {
                u = tu;
                v = tv;
            }
        }
        get_father(&self.father, &u, 0)
    }
    fn calc<T: DagNode, D: Dag<Node = T>>(&mut self) {
        let topo_len = self.topo.len();
        for j in 0..topo_len {
            let u = self.topo[j];
            let ve = get_edge_in_anti_graph(&self.anti_graph, &u);
            if ve.len() != 0 {
                let mut v = ve[0];
                for i in 1..ve.len() {
                    v = self.lca(v, ve[i]);
                }
                self.tree.insert(u, v);
                let v_add_one = get_dep_or_ind(&mut self.deep, &v) + 1;
                modify_dep(&mut self.deep, &u, v_add_one);
                set_father(&mut self.father, &u, 0, v);
                for i in 1..=self.scale {
                    let idx = &get_father(&self.father, &u, i - 1);
                    let id = get_father(&self.father, idx, i - 1);
                    set_father(&mut self.father, &u, i, id);
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
