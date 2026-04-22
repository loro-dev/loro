use std::collections::HashSet;

use crate::{
    dag::{Dag, DagNode},
    id::ID,
    version::Frontiers,
};

fn get_all_points<T: DagNode, D: Dag<Node = T>>(graph: &D, points: &mut HashSet<ID>, current: &ID) {
    points.insert(*current);
    for to_id in graph.get(*current).unwrap().deps().iter() {
        get_all_points(graph, points, &to_id);
    }
}

pub fn get_end_list<T: DagNode, D: Dag<Node = T>>(graph: &D, start_list: &Frontiers) -> Frontiers {
    let mut end_set: HashSet<ID> = HashSet::new();
    for start_id in start_list.iter() {
        end_dfs(graph, &start_id, &mut end_set);
    }
    end_set.into_iter().collect()
}

fn end_dfs<T: DagNode, D: Dag<Node = T>>(graph: &D, current: &ID, end_set: &mut HashSet<ID>) {
    let binding = graph.get(*current).unwrap();
    let deps = binding.deps();
    if deps.is_empty() {
        end_set.insert(*current);
    }
    for to_id in deps.iter() {
        end_dfs(graph, &to_id, end_set);
    }
}

pub fn calc_critical_version_dfs<T: DagNode, D: Dag<Node = T>>(
    graph: &D,
    start_list: &Frontiers,
    end_list: &Frontiers,
) -> Vec<ID> {
    let mut result: Vec<ID> = vec![];
    let mut points: HashSet<ID> = HashSet::new();
    let start_list_set: HashSet<ID> = HashSet::from_iter(start_list.iter());
    let end_list_set: HashSet<ID> = HashSet::from_iter(end_list.iter());
    for start_id in start_list.iter() {
        get_all_points(graph, &mut points, &start_id);
    }
    for escape in points {
        let mut flag = false;
        for start_id in start_list.iter() {
            if dfs(graph, &start_id, &escape, &end_list_set) {
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
    for to_id in graph.get(*current).unwrap().deps().iter() {
        if dfs(graph, &to_id, escape, end_list_set) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod additional_tests {
    use std::collections::BTreeMap;

    use loro_common::{HasId, HasIdSpan};
    use rle::{HasLength, Sliceable};

    use super::*;
    use crate::{
        change::Lamport,
        span::{HasLamport, HasLamportSpan},
        version::VersionVector,
    };

    #[derive(Clone, Debug)]
    struct TestNode {
        id: ID,
        lamport: Lamport,
        deps: Frontiers,
    }

    impl DagNode for TestNode {
        fn deps(&self) -> &Frontiers {
            &self.deps
        }
    }

    impl HasId for TestNode {
        fn id_start(&self) -> ID {
            self.id
        }
    }

    impl HasLamport for TestNode {
        fn lamport(&self) -> Lamport {
            self.lamport
        }
    }

    impl HasLength for TestNode {
        fn content_len(&self) -> usize {
            1
        }
    }

    impl Sliceable for TestNode {
        fn slice(&self, _from: usize, _to: usize) -> Self {
            self.clone()
        }
    }

    #[derive(Debug)]
    struct TestDag {
        nodes: BTreeMap<ID, TestNode>,
        vv: VersionVector,
        frontier: Frontiers,
    }

    impl TestDag {
        fn new(nodes: impl IntoIterator<Item = TestNode>, frontier: Frontiers) -> Self {
            let mut vv = VersionVector::default();
            let nodes = nodes
                .into_iter()
                .map(|node| {
                    vv.set_end(node.id_end());
                    (node.id_start(), node)
                })
                .collect();
            Self {
                nodes,
                vv,
                frontier,
            }
        }
    }

    impl Dag for TestDag {
        type Node = TestNode;

        fn get(&self, id: ID) -> Option<Self::Node> {
            self.nodes.get(&id).cloned()
        }

        fn frontier(&self) -> &Frontiers {
            &self.frontier
        }

        fn vv(&self) -> &VersionVector {
            &self.vv
        }

        fn contains(&self, id: ID) -> bool {
            self.nodes.contains_key(&id)
        }
    }

    fn node(peer: u64, counter: i32, lamport: Lamport, deps: Frontiers) -> TestNode {
        TestNode {
            id: ID::new(peer, counter),
            lamport,
            deps,
        }
    }

    #[test]
    fn end_list_collects_dependency_leaves_from_all_start_frontiers() {
        let a = node(1, 0, 0, Frontiers::default());
        let b = node(1, 1, 1, a.id.into());
        let c = node(2, 0, 2, a.id.into());
        let d = node(3, 0, 3, Frontiers::from([b.id, c.id]));
        let e = node(4, 0, 4, c.id.into());
        let dag = TestDag::new(
            vec![a.clone(), b.clone(), c.clone(), d.clone(), e.clone()],
            Frontiers::from([d.id, e.id]),
        );

        let ends = get_end_list(&dag, &Frontiers::from([d.id, e.id]));
        assert_eq!(ends, Frontiers::from(a.id));
    }

    #[test]
    fn dfs_critical_versions_include_linear_cut_points() {
        let root = node(1, 0, 0, Frontiers::default());
        let middle = node(1, 1, 1, root.id.into());
        let head = node(1, 2, 2, middle.id.into());
        let dag = TestDag::new(
            vec![root.clone(), middle.clone(), head.clone()],
            head.id.into(),
        );

        let critical = calc_critical_version_dfs(&dag, &head.id.into(), &root.id.into());
        assert_eq!(critical, vec![middle.id]);
    }

    #[test]
    fn dfs_critical_versions_exclude_diamond_branches_with_alternate_paths() {
        let root = node(1, 0, 0, Frontiers::default());
        let left = node(2, 0, 1, root.id.into());
        let right = node(3, 0, 2, root.id.into());
        let merge = node(4, 0, 3, Frontiers::from([left.id, right.id]));
        let dag = TestDag::new(
            vec![root.clone(), left.clone(), right.clone(), merge.clone()],
            merge.id.into(),
        );

        let critical = calc_critical_version_dfs(&dag, &merge.id.into(), &root.id.into());
        assert!(critical.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cmp::Ordering,
        collections::{HashMap, HashSet},
        sync::Arc,
    };

    use crate::{
        change::Lamport,
        id::{Counter, PeerID},
        span::{HasId, HasLamport},
    };
    use rle::{HasLength, Sliceable};

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestNode {
        id: ID,
        lamport: Lamport,
        len: usize,
        deps: Arc<Frontiers>,
    }

    impl TestNode {
        fn new(id: ID, lamport: Lamport, deps: Frontiers) -> Self {
            Self {
                id,
                lamport,
                len: 1,
                deps: Arc::new(deps),
            }
        }
    }

    impl DagNode for TestNode {
        fn deps(&self) -> &Frontiers {
            &self.deps
        }
    }

    impl Sliceable for TestNode {
        fn slice(&self, _from: usize, _to: usize) -> Self {
            self.clone()
        }
    }

    impl HasLamport for TestNode {
        fn lamport(&self) -> Lamport {
            self.lamport
        }
    }

    impl HasId for TestNode {
        fn id_start(&self) -> ID {
            self.id
        }
    }

    impl HasLength for TestNode {
        fn content_len(&self) -> usize {
            self.len
        }
    }

    #[derive(Debug)]
    struct TestDag {
        nodes: HashMap<PeerID, Vec<TestNode>>,
        version_vec: crate::version::VersionVector,
    }

    impl TestDag {
        fn new(nodes: Vec<TestNode>) -> Self {
            let mut map: HashMap<PeerID, Vec<TestNode>> = HashMap::new();
            let mut vv = crate::version::VersionVector::new();
            for node in nodes {
                vv.insert(node.id.peer, node.id.counter + node.len as Counter);
                map.entry(node.id.peer).or_default().push(node);
            }
            for nodes in map.values_mut() {
                nodes.sort_by(|a, b| match a.id.counter.cmp(&b.id.counter) {
                    Ordering::Equal => a.len.cmp(&b.len),
                    other => other,
                });
            }
            Self {
                nodes: map,
                version_vec: vv,
            }
        }
    }

    impl Dag for TestDag {
        type Node = TestNode;

        fn get(&self, id: ID) -> Option<Self::Node> {
            let arr = self.nodes.get(&id.peer)?;
            arr.binary_search_by(|node| {
                if node.id.counter > id.counter {
                    Ordering::Greater
                } else if node.id.counter + node.len as i32 <= id.counter {
                    Ordering::Less
                } else {
                    Ordering::Equal
                }
            })
            .ok()
            .map(|idx| arr[idx].clone())
        }

        fn frontier(&self) -> &Frontiers {
            panic!("frontier is not used in dfs tests")
        }

        fn vv(&self) -> &crate::version::VersionVector {
            &self.version_vec
        }

        fn contains(&self, id: ID) -> bool {
            self.version_vec.includes_id(id)
        }
    }

    fn id(peer: PeerID, counter: Counter) -> ID {
        ID::new(peer, counter)
    }

    fn frontier(ids: &[ID]) -> Frontiers {
        let mut frontier = Frontiers::new();
        for id in ids {
            frontier.push(*id);
        }
        frontier
    }

    fn as_set(ids: Vec<ID>) -> HashSet<ID> {
        ids.into_iter().collect()
    }

    #[test]
    fn get_end_list_collects_all_leaf_nodes_reachable_from_start() {
        let graph = TestDag::new(vec![
            TestNode::new(id(1, 0), 10, frontier(&[id(2, 0), id(3, 0)])),
            TestNode::new(id(2, 0), 7, Frontiers::new()),
            TestNode::new(id(3, 0), 8, Frontiers::new()),
        ]);

        let ends = get_end_list(&graph, &frontier(&[id(1, 0)]));

        assert_eq!(ends.len(), 2);
        assert!(ends.contains(&id(2, 0)));
        assert!(ends.contains(&id(3, 0)));
        assert!(!ends.contains(&id(1, 0)));
    }

    #[test]
    fn calc_critical_version_dfs_returns_non_start_nodes_when_no_end_is_present() {
        let graph = TestDag::new(vec![
            TestNode::new(id(1, 0), 10, frontier(&[id(2, 0), id(3, 0)])),
            TestNode::new(id(2, 0), 7, Frontiers::new()),
            TestNode::new(id(3, 0), 8, Frontiers::new()),
        ]);

        let result = calc_critical_version_dfs(&graph, &frontier(&[id(1, 0)]), &Frontiers::new());

        assert_eq!(as_set(result), as_set(vec![id(2, 0), id(3, 0)]));
    }

    #[test]
    fn calc_critical_version_dfs_skips_candidates_when_an_end_is_on_every_start_path() {
        let graph = TestDag::new(vec![
            TestNode::new(id(1, 0), 10, frontier(&[id(2, 0), id(3, 0)])),
            TestNode::new(id(2, 0), 7, Frontiers::new()),
            TestNode::new(id(3, 0), 8, Frontiers::new()),
        ]);

        let result =
            calc_critical_version_dfs(&graph, &frontier(&[id(1, 0)]), &frontier(&[id(2, 0)]));

        assert!(result.is_empty());
    }
}
