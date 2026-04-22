#![allow(dead_code)]
use crate::{
    dag::{Dag, DagNode},
    id::ID,
    version::Frontiers,
};

use rustc_hash::FxHashSet;
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
    fn sort_base_prioritizes_larger_lamport_in_binary_heap() {
        let mut heap = BinaryHeap::new();
        heap.push(SortBase {
            id: ID::new(1, 0),
            lamport: 10,
        });
        heap.push(SortBase {
            id: ID::new(2, 0),
            lamport: 20,
        });

        assert_eq!(heap.pop().unwrap().id, ID::new(2, 0));
        assert_eq!(heap.pop().unwrap().id, ID::new(1, 0));
    }

    #[test]
    fn bfs_critical_versions_include_linear_cut_points() {
        let root = node(1, 0, 0, Frontiers::default());
        let middle = node(1, 1, 1, root.id.into());
        let head = node(1, 2, 2, middle.id.into());
        let dag = TestDag::new(
            vec![root.clone(), middle.clone(), head.clone()],
            head.id.into(),
        );

        let critical = calc_critical_version_bfs(&dag, &head.id.into());
        assert_eq!(critical, vec![middle.id]);
    }

    #[test]
    fn bfs_critical_versions_skip_start_and_leaf_nodes_in_diamond_graphs() {
        let root = node(1, 0, 0, Frontiers::default());
        let left = node(2, 0, 1, root.id.into());
        let right = node(3, 0, 2, root.id.into());
        let merge = node(4, 0, 3, Frontiers::from([left.id, right.id]));
        let dag = TestDag::new(
            vec![root.clone(), left.clone(), right.clone(), merge.clone()],
            merge.id.into(),
        );

        let critical = calc_critical_version_bfs(&dag, &merge.id.into());
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
            panic!("frontier is not used in bfs tests")
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

    #[test]
    fn dedupes_shared_dependencies_and_filters_terminal_nodes() {
        let graph = TestDag::new(vec![
            TestNode::new(id(1, 0), 10, frontier(&[id(2, 0), id(3, 0)])),
            TestNode::new(id(2, 0), 8, frontier(&[id(4, 0)])),
            TestNode::new(id(3, 0), 7, frontier(&[id(4, 0)])),
            TestNode::new(id(4, 0), 1, Frontiers::new()),
        ]);

        let result = calc_critical_version_bfs(&graph, &frontier(&[id(1, 0)]));

        assert!(result.is_empty());
        assert_eq!(graph.get(id(4, 0)).unwrap().deps().len(), 0);
    }
}
