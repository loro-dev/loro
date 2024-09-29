use std::collections::HashSet;

use crate::{
    dag::{Dag, DagNode},
    id::ID,
};

fn get_all_points<T: DagNode, D: Dag<Node = T>>(graph: &D, points: &mut HashSet<ID>, current: &ID) {
    points.insert(*current);
    for to_id in graph.get(*current).unwrap().deps() {
        get_all_points(graph, points, to_id);
    }
}

pub fn get_end_list<T: DagNode, D: Dag<Node = T>>(graph: &D, start_list: &[ID]) -> Vec<ID> {
    let mut end_set: HashSet<ID> = HashSet::new();
    for start_id in start_list {
        end_dfs(graph, start_id, &mut end_set);
    }
    end_set.into_iter().collect()
}

fn end_dfs<T: DagNode, D: Dag<Node = T>>(graph: &D, current: &ID, end_set: &mut HashSet<ID>) {
    let binding = graph.get(*current).unwrap();
    let deps = binding.deps();
    if deps.is_empty() {
        end_set.insert(*current);
    }
    for to_id in deps {
        end_dfs(graph, to_id, end_set);
    }
}

pub fn calc_critical_version_dfs<T: DagNode, D: Dag<Node = T>>(
    graph: &D,
    start_list: &[ID],
    end_list: &[ID],
) -> Vec<ID> {
    let mut result: Vec<ID> = vec![];
    let mut points: HashSet<ID> = HashSet::new();
    let start_list_set: HashSet<ID> = HashSet::from_iter(start_list.iter().cloned());
    let end_list_set: HashSet<ID> = HashSet::from_iter(end_list.iter().cloned());
    for start_id in start_list {
        get_all_points(graph, &mut points, start_id);
    }
    for escape in points {
        let mut flag = false;
        for start_id in start_list {
            if dfs(graph, start_id, &escape, &end_list_set) {
                flag = true;
                break;
            }
        }
        if flag {
            continue;
        }
        if !end_list_set.contains(&escape) && !start_list_set.contains(&escape) {
            result.push(escape);
        }
    }
    result
}

fn dfs<T: DagNode, D: Dag<Node = T>>(
    graph: &D,
    current: &ID,
    escape: &ID,
    end_list_set: &HashSet<ID>,
) -> bool {
    if current == escape {
        return false;
    }
    if end_list_set.contains(current) {
        return true;
    }
    for to_id in graph.get(*current).unwrap().deps() {
        if dfs(graph, to_id, escape, end_list_set) {
            return true;
        }
    }
    false
}
