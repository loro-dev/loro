use crate::{
    dag::{Dag, DagNode},
    id::ID,
};
use fxhash::{FxHashMap, FxHashSet};

pub fn get_all_points<T: DagNode, D: Dag<Node = T>>(
    graph: &D,
    points: &mut FxHashSet<ID>,
    current: &ID,
) {
    points.insert(*current);
    for to_id in graph.get(*current).unwrap().deps() {
        get_all_points(graph, points, to_id);
    }
}

pub(crate) fn allocation_mermaid<T: DagNode, D: Dag<Node = T>>(
    graph: &D,
    start_id_list: &[ID],
    end_id_list: &[ID],
) -> String {
    let mut s = String::new();
    s.push_str("graph TD\n");
    let mut counter: u32 = 2;
    let mut points: FxHashSet<ID> = FxHashSet::default();
    for start in start_id_list {
        get_all_points(graph, &mut points, &start);
    }
    let mut counter_map: FxHashMap<ID, u32> = FxHashMap::default();
    for x in points {
        counter_map.insert(x, counter);
        counter += 1;
    }
    for start in start_id_list {
        s.push_str(&format!(
            "\t1(virtual_start) --> {}(id:{} lamport:{})\n",
            counter_map.get(&start).unwrap(),
            start,
            graph.get(*start).unwrap().lamport()
        ));
    }
    for end in end_id_list {
        s.push_str(&format!(
            "\t{}(id:{} lamport:{}) --> 114514(virtual_end)\n",
            counter_map.get(&end).unwrap(),
            end,
            graph.get(*end).unwrap().lamport()
        ));
        counter += 1;
    }
    let mut edge: FxHashSet<(ID, ID)> = FxHashSet::default();
    for start in start_id_list {
        dfs(
            graph,
            ID {
                peer: 0,
                counter: -1,
            },
            *start,
            &mut edge,
        );
    }
    for (from, to) in edge {
        s.push_str(&format!(
            "\t{}(id:{} lamport:{}) --> {}(id:{} lamport:{})\n",
            counter_map.get(&from).unwrap(),
            from,
            graph.get(from).unwrap().lamport(),
            counter_map.get(&to).unwrap(),
            to,
            graph.get(to).unwrap().lamport()
        ));
    }
    s
}

pub(crate) fn dfs<T: DagNode, D: Dag<Node = T>>(
    graph: &D,
    from: ID,
    id: ID,
    edge: &mut FxHashSet<(ID, ID)>,
) {
    if from.counter != -1 {
        edge.insert((from, id));
    }
    for dep in graph.get(id).unwrap().deps() {
        dfs(graph, id, *dep, edge);
    }
}
