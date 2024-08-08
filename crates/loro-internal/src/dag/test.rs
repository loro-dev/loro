use loro_common::HasCounter;
use proptest::prelude::*;
use std::cmp::Ordering;

use super::*;
use crate::{
    array_mut_ref,
    change::Lamport,
    id::{Counter, PeerID, ID},
    span::HasIdSpan,
};

#[derive(Debug, PartialEq, Eq, Clone)]
struct TestNode {
    id: ID,
    lamport: Lamport,
    len: usize,
    deps: Vec<ID>,
}

impl TestNode {
    fn new(id: ID, lamport: Lamport, deps: Vec<ID>, len: usize) -> Self {
        Self {
            id,
            lamport,
            deps,
            len,
        }
    }
}

impl DagNode for TestNode {
    fn deps(&self) -> &[ID] {
        &self.deps
    }
}

impl Sliceable for TestNode {
    fn slice(&self, from: usize, to: usize) -> Self {
        Self {
            id: self.id.inc(from as Counter),
            lamport: self.lamport + from as Lamport,
            len: to - from,
            deps: if from > 0 {
                vec![self.id.inc(from as Counter - 1)]
            } else {
                self.deps.clone()
            },
        }
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

impl HasCounter for TestNode {
    fn ctr_start(&self) -> Counter {
        self.id.counter
    }
}

impl HasLength for TestNode {
    fn content_len(&self) -> usize {
        self.len
    }
}

#[derive(Debug, PartialEq, Eq)]
struct TestDag {
    nodes: FxHashMap<PeerID, Vec<TestNode>>,
    frontier: Vec<ID>,
    version_vec: VersionVector,
    next_lamport: Lamport,
    client_id: PeerID,
}

impl TestDag {
    fn is_first(&self) -> bool {
        *self.version_vec.get(&self.client_id).unwrap_or(&0) == 0
    }
}

impl Dag for TestDag {
    type Node = TestNode;

    fn get(&self, id: ID) -> Option<&Self::Node> {
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
        .map_or(None, |x| Some(&arr[x]))
    }

    fn frontier(&self) -> &[ID] {
        &self.frontier
    }

    fn vv(&self) -> VersionVector {
        self.version_vec.clone()
    }
}

impl TestDag {
    pub fn new(client_id: PeerID) -> Self {
        Self {
            nodes: FxHashMap::default(),
            frontier: Vec::new(),
            version_vec: VersionVector::new(),
            next_lamport: 0,
            client_id,
        }
    }

    fn get_last_node(&mut self) -> &mut TestNode {
        self.nodes
            .get_mut(&self.client_id)
            .unwrap()
            .last_mut()
            .unwrap()
    }

    fn push(&mut self, len: usize) {
        let client_id = self.client_id;
        let counter = self.version_vec.entry(client_id).or_insert(0);
        let id = ID::new(client_id, *counter);
        *counter += len as Counter;
        let deps = std::mem::replace(&mut self.frontier, vec![id.inc(len as Counter - 1)]);
        self.nodes.entry(client_id).or_default().push(TestNode::new(
            id,
            self.next_lamport,
            deps,
            len,
        ));
        self.next_lamport += len as u32;
    }

    fn merge(&mut self, other: &TestDag) {
        let mut pending = Vec::new();
        for (_, nodes) in other.nodes.iter() {
            for (i, node) in nodes.iter().enumerate() {
                if self._try_push_node(node, &mut pending, i) {
                    break;
                }
            }
        }

        let mut current = pending;
        let mut pending = Vec::new();
        while !pending.is_empty() || !current.is_empty() {
            if current.is_empty() {
                std::mem::swap(&mut pending, &mut current);
            }

            let (client_id, index) = current.pop().unwrap();
            let node_vec = other.nodes.get(&client_id).unwrap();
            #[allow(clippy::needless_range_loop)]
            for i in index..node_vec.len() {
                let node = &node_vec[i];
                if self._try_push_node(node, &mut pending, i) {
                    break;
                }
            }
        }
    }

    fn update_frontier(frontier: &mut Vec<ID>, new_node_id: ID, new_node_deps: &[ID]) {
        frontier.retain(|x| {
            if x.peer == new_node_id.peer && x.counter <= new_node_id.counter {
                return false;
            }

            !new_node_deps
                .iter()
                .any(|y| y.peer == x.peer && y.counter >= x.counter)
        });

        // nodes from the same client with `counter < new_node_id.counter`
        // are filtered out from frontier.
        if frontier.iter().all(|x| x.peer != new_node_id.peer) {
            frontier.push(new_node_id);
        }
    }

    fn _try_push_node(
        &mut self,
        node: &TestNode,
        pending: &mut Vec<(PeerID, usize)>,
        i: usize,
    ) -> bool {
        let client_id = node.id.peer;
        if self.contains(node.id_last()) {
            return false;
        }
        if node.deps.iter().any(|dep| !self.contains(*dep)) {
            pending.push((client_id, i));
            return true;
        }
        Self::update_frontier(&mut self.frontier, node.id_last(), &node.deps);
        let contains_start = self.contains(node.id_start());
        let arr = self.nodes.entry(client_id).or_default();
        if contains_start {
            arr.pop();
            arr.push(node.clone());
        } else {
            arr.push(node.clone());
        }
        self.version_vec.set_end(node.id_end());
        self.next_lamport = self.next_lamport.max(node.lamport + node.len as u32);
        false
    }
}

/// ```mermaid /// flowchart RL
/// subgraph client0
/// 0-1("c0: [1, 3)") --> 0-0("c0: [0, 1)")
/// end
///
/// subgraph client1
/// 1-0("c1: [0, 2)")
/// end
///
/// 0-1 --> 1-0
/// ```
#[test]
fn test_dag() {
    let mut a = TestDag::new(0);
    let mut b = TestDag::new(1);
    a.push(1);
    assert_eq!(a.frontier().len(), 1);
    assert_eq!(a.frontier()[0].counter, 0);
    b.push(1);
    a.merge(&b);
    assert_eq!(a.frontier().len(), 2);
    a.push(1);
    assert_eq!(a.frontier().len(), 1);
    // a:   0 --(merge)--- 1
    //            ↑
    //            |
    // b:   0 ----
    assert_eq!(
        a.frontier()[0],
        ID {
            peer: 0,
            counter: 1
        }
    );

    // a:   0 --(merge)--- 1 --- 2 -------
    //            ↑                      |
    //            |                     ↓
    // b:   0 ------------1----------(merge)
    a.push(1);
    b.push(1);
    b.merge(&a);
    assert_eq!(b.next_lamport, 3);
    assert_eq!(b.frontier().len(), 2);
    // println!("{}", b.mermaid());
    assert_eq!(
        b.find_common_ancestor(&[ID::new(0, 2)], &[ID::new(1, 1)])
            .first()
            .copied(),
        None,
    );
}

#[derive(Debug, Clone, Copy)]
struct Interaction {
    dag_idx: usize,
    merge_with: Option<usize>,
    len: usize,
}

impl Interaction {
    fn generate(rng: &mut impl rand::Rng, num: usize) -> Self {
        if rng.gen_bool(0.5) {
            let dag_idx = rng.gen_range(0..num);
            let merge_with = (rng.gen_range(1..num - 1) + dag_idx) % num;
            Self {
                dag_idx,
                merge_with: Some(merge_with),
                len: rng.gen_range(1..10),
            }
        } else {
            Self {
                dag_idx: rng.gen_range(0..num),
                merge_with: None,
                len: rng.gen_range(1..10),
            }
        }
    }

    fn apply(&self, dags: &mut [TestDag]) {
        if let Some(merge_with) = self.merge_with {
            if merge_with != self.dag_idx {
                let (from, to) = array_mut_ref!(dags, [self.dag_idx, merge_with]);
                from.merge(to);
            }
        }

        dags[self.dag_idx].push(self.len);
    }
}

prop_compose! {
    fn gen_interaction(num: usize) (
            dag_idx in 0..num,
            merge_with in 0..num,
            length in 1..10,
            should_merge in 0..2
        ) -> Interaction {
        Interaction {
            dag_idx,
            merge_with: if should_merge == 1 && merge_with != dag_idx { Some(merge_with) } else { None },
            len: length as usize,
        }
    }
}

fn preprocess(interactions: &mut [Interaction], num: i32) {
    for interaction in interactions.iter_mut() {
        interaction.dag_idx %= num as usize;
        if let Some(ref mut merge_with) = interaction.merge_with {
            *merge_with %= num as usize;
            if *merge_with == interaction.dag_idx {
                *merge_with = (*merge_with + 1) % num as usize;
            }
        }
    }
}

mod iter {
    use super::*;

    #[test]
    fn test() {
        let mut a = TestDag::new(0);
        let mut b = TestDag::new(1);
        // 0-0
        a.push(1);
        // 1-0
        b.push(1);
        a.merge(&b);
        // 0-1
        a.push(1);
        b.merge(&a);
        // 1-1
        b.push(1);
        a.merge(&b);
        // 0-2
        a.push(1);

        let mut count = 0;
        for (node, vv) in a.iter_with_vv() {
            count += 1;
            if node.id == ID::new(0, 0) {
                assert_eq!(vv, vec![ID::new(0, 0)].into());
            } else if node.id == ID::new(0, 2) {
                assert_eq!(vv, vec![ID::new(0, 2), ID::new(1, 1)].into());
            }
        }

        assert_eq!(count, 5);
    }
}

mod allocation_tree {
    use super::*;
    use crate::{allocation::calc_critical_version_allocation_tree, delta::DeltaValue};
    use rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn test_alloc_tree_small() {
        let mut a = TestDag::new(0);
        let mut b = TestDag::new(1);
        let mut c = TestDag::new(2);
        a.push(10);
        b.merge(&a);
        b.push(3);
        c.merge(&b);
        c.push(4);
        a.push(3);
        a.merge(&c);
        a.push(2);
        b.merge(&a);
        assert_eq!(
            calc_critical_version_allocation_tree::<TestNode, TestDag>(
                &b,
                &[ID {
                    peer: 0,
                    counter: 13,
                }],
                &[ID {
                    peer: 0,
                    counter: 9,
                }],
            ),
            vec![
                ID {
                    peer: 0,
                    counter: 9,
                },
                ID {
                    peer: 0,
                    counter: 13,
                },
            ]
        );
    }

    #[test]
    fn test_alloc_tree_big() {
        let num = 5;
        let mut rng = StdRng::seed_from_u64(100);
        let mut dags = (0..num).map(TestDag::new).collect::<Vec<_>>();
        for _ in 0..100 {
            Interaction::generate(&mut rng, num as usize).apply(&mut dags);
        }
        for i in 1..num {
            let (a, other) = array_mut_ref!(&mut dags, [0, i as usize]);
            a.merge(other);
        }
        let start = dags[0].frontier();
        let ends = [
            ID {
                peer: 3,
                counter: 7,
            },
            ID {
                peer: 4,
                counter: 6,
            },
        ];
        assert_eq!(
            calc_critical_version_allocation_tree(&dags[0], start, &ends).length(),
            0
        );
    }
}

mod lamport_split {
    use super::*;
    use crate::{allocation::calc_critical_version_lamport_split, delta::DeltaValue};
    use rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn test_lamport_split_small() {
        let mut a = TestDag::new(0);
        let mut b = TestDag::new(1);
        let mut c = TestDag::new(2);
        a.push(10);
        b.merge(&a);
        b.push(3);
        c.merge(&b);
        c.push(4);
        a.push(3);
        a.merge(&c);
        a.push(2);
        b.merge(&a);
        assert_eq!(
            calc_critical_version_lamport_split::<TestNode, TestDag>(
                &b,
                &[ID {
                    peer: 0,
                    counter: 13,
                }],
                &[ID {
                    peer: 0,
                    counter: 9,
                }],
            )
            .length(),
            0
        );
    }

    #[test]
    fn test_lamport_split_big() {
        let num = 5;
        let mut rng = StdRng::seed_from_u64(100);
        let mut dags = (0..num).map(TestDag::new).collect::<Vec<_>>();
        for _ in 0..100 {
            Interaction::generate(&mut rng, num as usize).apply(&mut dags);
        }
        for i in 1..num {
            let (a, other) = array_mut_ref!(&mut dags, [0, i as usize]);
            a.merge(other);
        }
        let start = dags[0].frontier();
        let ends = [
            ID {
                peer: 3,
                counter: 7,
            },
            ID {
                peer: 4,
                counter: 6,
            },
        ];
        assert_eq!(
            calc_critical_version_lamport_split(&dags[0], start, &ends).length(),
            0
        );
    }
}

mod mermaid {

    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn simple() {
        let mut a = TestDag::new(0);
        let mut b = TestDag::new(1);
        // 0-0
        a.push(1);
        // 1-0
        b.push(1);
        a.merge(&b);
        // 0-1
        a.push(1);
        b.merge(&a);
        // 1-1
        b.push(1);
        a.merge(&b);
        // 0-2
        a.push(1);

        println!("{}", a.mermaid());
    }

    #[test]
    fn three() {
        let mut a = TestDag::new(0);
        let mut b = TestDag::new(1);
        let mut c = TestDag::new(2);
        a.push(10);
        b.merge(&a);
        b.push(3);
        c.merge(&b);
        c.push(4);
        a.push(3);
        a.merge(&c);
        a.push(2);
        b.merge(&a);
        println!("{}", a.mermaid());
    }

    #[test]
    fn gen_graph() {
        let num = 5;
        let mut rng = StdRng::seed_from_u64(100);
        let mut dags = (0..num).map(TestDag::new).collect::<Vec<_>>();
        for _ in 0..100 {
            Interaction::generate(&mut rng, num as usize).apply(&mut dags);
        }
        for i in 1..num {
            let (a, other) = array_mut_ref!(&mut dags, [0, i as usize]);
            a.merge(other);
        }
        println!("{}", dags[0].mermaid());
    }
}

mod get_version_vector {
    use super::*;

    #[test]
    fn vv() {
        let mut a = TestDag::new(0);
        let mut b = TestDag::new(1);
        a.push(1);
        b.push(1);
        a.merge(&b);
        a.push(1);
        let actual = a.get_vv(ID::new(0, 0));
        assert_eq!(actual, vec![ID::new(0, 0)].into());
        let actual = a.get_vv(ID::new(0, 1));
        assert_eq!(actual, vec![ID::new(0, 1), ID::new(1, 0)].into());

        let mut c = TestDag::new(2);
        c.merge(&a);
        b.push(1);
        c.merge(&b);
        c.push(1);
        let actual = c.get_vv(ID::new(2, 0));
        assert_eq!(
            actual,
            vec![ID::new(0, 1), ID::new(1, 1), ID::new(2, 0)].into()
        );
    }
}

#[cfg(test)]
mod find_path {
    use crate::{fx_map, span::CounterSpan, tests::PROPTEST_FACTOR_10};

    use super::*;

    #[test]
    fn retreat_to_beginning() {
        let mut a = TestDag::new(0);
        let mut b = TestDag::new(1);
        a.push(1);
        b.push(1);
        a.merge(&b);
        let actual = a.find_path(&[ID::new(0, 0)], &[ID::new(1, 0)]);
        assert_eq!(
            actual,
            VersionVectorDiff {
                left: fx_map!(0 => CounterSpan { start: 0, end: 1 }),
                right: fx_map!(1 => CounterSpan { start: 0, end: 1 })
            }
        );
    }

    #[test]
    fn one_path() {
        let mut a = TestDag::new(0);
        let mut b = TestDag::new(1);
        // 0 - 0
        a.push(1);
        b.merge(&a);
        // 1 - 0
        b.push(1);
        // 0 - 1
        a.push(1);
        a.merge(&b);
        let actual = a.find_path(&[ID::new(0, 1)], &[ID::new(1, 0)]);
        assert_eq!(
            actual,
            VersionVectorDiff {
                left: fx_map!(0 => CounterSpan { start: 1, end: 2 }),
                right: fx_map!(1 => CounterSpan { start: 0, end: 1 })
            }
        );
    }

    #[test]
    fn middle() {
        let mut a = TestDag::new(0);
        let mut b = TestDag::new(1);
        a.push(2);
        b.push(1);
        b.merge(&a);
        b.push(2);
        a.push(2);
        b.merge(&a);
        // println!("{}", b.mermaid());
        let actual = b.find_path(&[ID::new(0, 3)], &[ID::new(1, 2)]);
        assert_eq!(
            actual,
            VersionVectorDiff {
                left: fx_map!(0 => CounterSpan { start: 2, end: 4 }),
                right: fx_map!(1 => CounterSpan { start: 0, end: 3 })
            }
        );
        let actual = b.find_path(&[ID::new(1, 1), ID::new(1, 2)], &[ID::new(0, 3)]);
        assert_eq!(
            actual,
            VersionVectorDiff {
                left: fx_map!(1 => CounterSpan { start: 0, end: 3 }),
                right: fx_map!(0 => CounterSpan { start: 2, end: 4 })
            }
        );
    }

    fn test_find_path(
        dag_num: i32,
        mut interactions: Vec<Interaction>,
    ) -> Result<(), TestCaseError> {
        preprocess(&mut interactions, dag_num);
        let mut dags = Vec::new();
        for i in 0..dag_num {
            dags.push(TestDag::new(i as PeerID));
        }

        for interaction in interactions.iter_mut() {
            interaction.apply(&mut dags);
        }

        for i in 1..dag_num {
            let (a, b) = array_mut_ref!(&mut dags, [0, i as usize]);
            a.merge(b);
        }

        let a = &dags[0];
        let mut nodes = Vec::new();
        for (node, vv) in a.iter_with_vv() {
            nodes.push((node, vv));
        }

        // println!("{}", a.mermaid());
        let vec: Vec<_> = nodes.iter().enumerate().collect();
        for &(i, (node, vv)) in vec.iter() {
            if i > 3 {
                break;
            }

            for &(j, (other_node, other_vv)) in vec.iter() {
                if i >= j {
                    continue;
                }

                let actual = a.find_path(&[node.id], &[other_node.id]);
                let expected = vv.diff(other_vv);
                prop_assert_eq!(
                    actual,
                    expected,
                    "\ni={} j={} node={} other={}",
                    i,
                    j,
                    node.id,
                    other_node.id
                );

                for iter in nodes[j + 1..].iter() {
                    let mut vv = vv.clone();
                    vv.merge(other_vv);
                    let actual = a.find_path(&[node.id, other_node.id], &[iter.0.id]);
                    let expected = vv.diff(&iter.1);
                    prop_assert_eq!(actual, expected);
                }
            }
        }

        Ok(())
    }

    #[test]
    fn issue() {
        if let Err(err) = test_find_path(
            5,
            vec![
                Interaction {
                    dag_idx: 1,
                    merge_with: None,
                    len: 3,
                },
                Interaction {
                    dag_idx: 1,
                    merge_with: None,
                    len: 3,
                },
                Interaction {
                    dag_idx: 4,
                    merge_with: Some(1),
                    len: 1,
                },
                Interaction {
                    dag_idx: 0,
                    merge_with: None,
                    len: 1,
                },
                Interaction {
                    dag_idx: 2,
                    merge_with: Some(0),
                    len: 1,
                },
                Interaction {
                    dag_idx: 0,
                    merge_with: None,
                    len: 1,
                },
                Interaction {
                    dag_idx: 2,
                    merge_with: Some(0),
                    len: 1,
                },
                Interaction {
                    dag_idx: 3,
                    merge_with: Some(0),
                    len: 1,
                },
            ],
        ) {
            panic!("{}", err);
        }
    }

    proptest! {
        #[test]
        fn proptest_path(
            interactions in prop::collection::vec(gen_interaction(5), 0..10 * PROPTEST_FACTOR_10),
        ) {
            test_find_path(5, interactions)?;
        }

        #[test]
        fn proptest_path_large(
            interactions in prop::collection::vec(gen_interaction(10), 0..PROPTEST_FACTOR_10 * PROPTEST_FACTOR_10 + 10),
        ) {
            test_find_path(10, interactions)?;
        }
    }
}

mod find_common_ancestors {

    use super::*;

    #[test]
    fn siblings() {
        let mut a = TestDag::new(0);
        a.push(5);
        let actual = a
            .find_common_ancestor(&[ID::new(0, 2)], &[ID::new(0, 4)])
            .first()
            .copied();
        assert_eq!(actual, Some(ID::new(0, 2)));
    }

    #[test]
    fn no_common_ancestors() {
        let mut a = TestDag::new(0);
        let mut b = TestDag::new(1);
        a.push(1);
        b.push(1);
        a.merge(&b);
        let actual = a
            .find_common_ancestor(&[ID::new(0, 0)], &[ID::new(1, 0)])
            .first()
            .copied();
        assert_eq!(actual, None);

        // interactions between b and c
        let mut c = TestDag::new(2);
        c.merge(&b);
        c.push(2);
        b.merge(&c);
        b.push(3);

        // should no exist any common ancestor between a and b
        let actual = a
            .find_common_ancestor(&[ID::new(0, 0)], &[ID::new(1, 0)])
            .first()
            .copied();
        assert_eq!(actual, None);
    }

    #[test]
    fn no_common_ancestors_when_there_is_an_redundant_node() {
        let mut a = TestDag::new(0);
        let mut b = TestDag::new(1);
        a.push(1);
        b.push(1);
        b.merge(&a);
        b.push(1);
        a.push(4);
        a.merge(&b);
        println!("{}", a.mermaid());
        let actual = a
            .find_common_ancestor(&[ID::new(0, 4)], &[ID::new(0, 1), ID::new(1, 1)])
            .first()
            .copied();
        assert_eq!(actual, None);
        let actual = a
            .find_common_ancestor(&[ID::new(0, 4)], &[ID::new(1, 1)])
            .first()
            .copied();
        assert_eq!(actual, None);
    }

    #[test]
    fn dep_in_middle() {
        let mut a = TestDag::new(0);
        let mut b = TestDag::new(1);
        a.push(3);
        b.merge(&a);
        b.push(9);
        a.push(2);
        b.merge(&a);
        println!("{}", b.mermaid());
        assert_eq!(
            b.find_common_ancestor(&[ID::new(0, 3)], &[ID::new(1, 8)])
                .first()
                .copied(),
            Some(ID::new(0, 2))
        );
    }

    /// ![](https://mermaid.ink/img/pako:eNqNkTFPwzAQhf_K6SYqOZJ9CYsHJroxwYgXY7skInEq1xFCVf87jg5XVQQSnk6fz_feO5_RzT6gxsM4f7repgzPTyZCOafl7T3ZYw9uHELMkqls2juDTmp4bQV0O4M7aJqHwqlyEtDecFW5EkA3XFYuBaiVs0CInotfXSimqunW16q87gTcX6cqdqe27hSrKVZr_6tGTImn0nYqcWbaZiZWI1ajP9WK2zqnClFd5jVn3SIKnEKa7ODLb53Xa4O5D1MwqEvpbfowaOKl9C1Hb3PY-yHPCfXBjqcg0C55fvmKDnVOS6hNj4Mtgaefrss3dp6HFg)
    #[test]
    fn large_lamport_with_longer_path() {
        let mut a0 = TestDag::new(0);
        let mut a1 = TestDag::new(1);
        let mut a2 = TestDag::new(2);

        a0.push(3);
        a1.merge(&a0);
        a2.merge(&a0);
        a1.push(3);
        a2.push(2);
        a2.push(1);
        a1.merge(&a2);
        a2.push(1);
        a1.push(1);
        a1.merge(&a2);
        a1.push(1);
        a0.push(1);
        a1.merge(&a2);
        a1.merge(&a0);
        println!("{}", a1.mermaid());
        assert_eq!(
            a1.find_common_ancestor(&[ID::new(0, 3)], &[ID::new(1, 4)])
                .first()
                .copied(),
            Some(ID::new(0, 2))
        );
        assert_eq!(
            a1.find_common_ancestor(&[ID::new(2, 3)], &[ID::new(1, 3)])
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            vec![ID::new(0, 2)]
        );
    }
}

mod find_common_ancestors_proptest {

    use crate::{
        array_mut_ref,
        span::HasIdSpan,
        tests::{PROPTEST_FACTOR_1, PROPTEST_FACTOR_10},
    };

    use super::*;

    proptest! {
        #[test]
        fn test_2dags(
            before_merged_insertions in prop::collection::vec(gen_interaction(2), 0..100 * PROPTEST_FACTOR_10),
            after_merged_insertions in prop::collection::vec(gen_interaction(2), 0..100 * PROPTEST_FACTOR_10)
        ) {
            test_single_common_ancestor(2, before_merged_insertions, after_merged_insertions)?;
        }

        #[test]
        fn test_4dags(
            before_merged_insertions in prop::collection::vec(gen_interaction(4), 0..50 * PROPTEST_FACTOR_10),
            after_merged_insertions in prop::collection::vec(gen_interaction(4), 0..50 * PROPTEST_FACTOR_10)
        ) {
            test_single_common_ancestor(4, before_merged_insertions, after_merged_insertions)?;
        }

        #[test]
        fn test_10dags(
            before_merged_insertions in prop::collection::vec(gen_interaction(10), 0..10 * PROPTEST_FACTOR_10 * PROPTEST_FACTOR_10),
            after_merged_insertions in prop::collection::vec(gen_interaction(10), 0..10 * PROPTEST_FACTOR_10 * PROPTEST_FACTOR_10)
        ) {
            test_single_common_ancestor(10, before_merged_insertions, after_merged_insertions)?;
        }

        #[test]
        fn test_mul_ancestors_5dags(
            before_merged_insertions in prop::collection::vec(gen_interaction(10), 0..30 + PROPTEST_FACTOR_1 * 500),
            after_merged_insertions in prop::collection::vec(gen_interaction(10), 0..30 + PROPTEST_FACTOR_1 * 500)
        ) {
            test_mul_ancestors::<2>(5, before_merged_insertions, after_merged_insertions)?;
        }

        #[test]
        fn test_mul_ancestors_10dags(
            before_merged_insertions in prop::collection::vec(gen_interaction(10), 0..10 * PROPTEST_FACTOR_10 * PROPTEST_FACTOR_10),
            after_merged_insertions in prop::collection::vec(gen_interaction(10), 0..10 * PROPTEST_FACTOR_10 * PROPTEST_FACTOR_10)
        ) {
            test_mul_ancestors::<3>(10, before_merged_insertions, after_merged_insertions)?;
        }

        #[test]
        fn test_mul_ancestors_15dags_2(
            before_merged_insertions in prop::collection::vec(gen_interaction(15), 0..10 * PROPTEST_FACTOR_10 * PROPTEST_FACTOR_10),
            after_merged_insertions in prop::collection::vec(gen_interaction(15), 0..10 * PROPTEST_FACTOR_10 * PROPTEST_FACTOR_10)
        ) {
            test_mul_ancestors::<5>(15, before_merged_insertions, after_merged_insertions)?;
        }
    }

    #[test]
    fn issue_0() {
        if let Err(err) = test_mul_ancestors::<3>(
            10,
            vec![
                Interaction {
                    dag_idx: 1,
                    merge_with: None,
                    len: 1,
                },
                Interaction {
                    dag_idx: 8,
                    merge_with: None,
                    len: 2,
                },
            ],
            vec![Interaction {
                dag_idx: 1,
                merge_with: None,
                len: 1,
            }],
        ) {
            println!("{}", err);
            panic!();
        }
    }

    #[test]
    fn issue_1() {
        test_single_common_ancestor(
            2,
            vec![],
            vec![
                Interaction {
                    dag_idx: 0,
                    merge_with: None,
                    len: 1,
                },
                Interaction {
                    dag_idx: 0,
                    merge_with: None,
                    len: 1,
                },
            ],
        )
        .unwrap();
    }

    fn test_single_common_ancestor(
        dag_num: i32,
        mut before_merge_insertion: Vec<Interaction>,
        mut after_merge_insertion: Vec<Interaction>,
    ) -> Result<(), TestCaseError> {
        preprocess(&mut before_merge_insertion, dag_num);
        preprocess(&mut after_merge_insertion, dag_num);
        let mut dags = Vec::new();
        for i in 0..dag_num {
            dags.push(TestDag::new(i as PeerID));
        }

        for interaction in before_merge_insertion {
            apply(interaction, &mut dags);
        }

        for dag_idx in 1..dags.len() {
            let (dag0, dag) = arref::array_mut_ref!(&mut dags, [0, dag_idx]);
            dag0.merge(dag);
        }

        dags[0].push(1);
        let expected = dags[0].frontier()[0];
        for dag_idx in 1..dags.len() {
            let (dag0, dag) = arref::array_mut_ref!(&mut dags, [0, dag_idx]);
            dag.merge(dag0);
        }
        for interaction in after_merge_insertion.iter_mut() {
            if let Some(merge) = interaction.merge_with {
                // odd dag merges with the odd
                // even dag merges with the even
                if merge % 2 != interaction.dag_idx % 2 {
                    interaction.merge_with = None;
                }
            }

            apply(*interaction, &mut dags);
        }

        let (dag0, dag1) = array_mut_ref!(&mut dags, [0, 1]);
        dag1.push(1);
        dag0.merge(dag1);
        // println!("{}", dag0.mermaid());
        let a = dags[0].nodes.get(&0).unwrap().last().unwrap().id_last();
        let b = dags[1].nodes.get(&1).unwrap().last().unwrap().id_last();
        let actual = dags[0].find_common_ancestor(&[a], &[b]);
        prop_assert_eq!(&**actual, &[expected]);
        Ok(())
    }

    fn apply(interaction: Interaction, dags: &mut [TestDag]) {
        let Interaction {
            dag_idx,
            len,
            merge_with,
        } = interaction;
        if let Some(merge_with) = merge_with {
            let (dag, merge_target): (&mut TestDag, &mut TestDag) =
                array_mut_ref!(dags, [dag_idx, merge_with]);
            dag.push(len);
            dag.merge(merge_target);
        } else {
            dags[dag_idx].push(len);
        }
    }

    fn test_mul_ancestors<const N: usize>(
        dag_num: i32,
        mut before_merge_insertion: Vec<Interaction>,
        mut after_merge_insertion: Vec<Interaction>,
    ) -> Result<(), TestCaseError> {
        assert!(dag_num - 2 >= N as i32);
        preprocess(&mut before_merge_insertion, dag_num);
        preprocess(&mut after_merge_insertion, dag_num);
        let mut dags = Vec::new();
        for i in 0..dag_num {
            dags.push(TestDag::new(i as PeerID));
        }

        for mut interaction in before_merge_insertion {
            if interaction.dag_idx < N {
                // cannot act on first N nodes
                interaction.dag_idx = interaction.dag_idx % (dags.len() - N) + N;
            }
            if let Some(merge) = interaction.merge_with {
                if interaction.dag_idx == merge {
                    let next_merge = (merge + 1) % dags.len();
                    interaction.merge_with = Some(next_merge);
                }
            }

            apply(interaction, &mut dags);
        }

        for target in 0..N {
            for i in N..dags.len() {
                let (target, dag): (&mut TestDag, &mut TestDag) =
                    arref::array_mut_ref!(&mut dags, [target, i]);
                target.merge(dag);
            }
        }

        let mut expected = Vec::with_capacity(N);
        for dag in dags[0..N].iter_mut() {
            dag.push(1);
            expected.push(dag.frontier[0]);
        }

        for target in 0..N {
            for i in N..dags.len() {
                let (target, dag): (&mut TestDag, &mut TestDag) =
                    arref::array_mut_ref!(&mut dags, [target, i]);
                dag.merge(target);
            }
        }

        let mut merged_to_even = [false; N];
        let mut merged_to_odd = [false; N];

        for interaction in after_merge_insertion.iter_mut() {
            if interaction.dag_idx < N {
                // cannot act on first N nodes
                interaction.dag_idx = interaction.dag_idx % (dags.len() - N) + N;
            }
            if let Some(mut merge) = interaction.merge_with {
                if interaction.dag_idx == merge {
                    let next_merge = (merge + 1) % dags.len();
                    interaction.merge_with = Some(next_merge);
                    merge = next_merge;
                }

                // odd dag merges with the odd
                // even dag merges with the even
                if merge >= N && merge % 2 != interaction.dag_idx % 2 {
                    interaction.merge_with = None;
                }
                if merge < N {
                    if interaction.dag_idx % 2 == 0 {
                        merged_to_even[merge] = true;
                    } else {
                        merged_to_odd[merge] = true;
                    }
                }
            }

            if dags[interaction.dag_idx].is_first() {
                // need to merge to one of the common ancestors first
                let target = interaction.dag_idx % N;
                let (dag, target) = arref::array_mut_ref!(&mut dags, [interaction.dag_idx, target]);
                dag.merge(target);
            }

            apply(*interaction, &mut dags);
        }

        // make common ancestor dags be merged to opposite side (even/odd)
        for i in 0..N {
            let (odd, even) = if N % 2 == 0 { (N + 1, N) } else { (N, N + 1) };
            if !merged_to_even[i] && i % 2 != 0 {
                let (dag_a, dag_b) = array_mut_ref!(&mut dags, [even, i]);
                dag_a.merge(dag_b);
                dag_a.push(1);
            }
            if !merged_to_odd[i] && i % 2 == 0 {
                let (dag_a, dag_b) = array_mut_ref!(&mut dags, [odd, i]);
                dag_a.merge(dag_b);
                dag_a.push(1);
            }
        }

        // merge with all odds or evens
        for i in dags.len() - 2..dags.len() {
            if i % 2 == 0 {
                for target in (0..dags.len() - 2).step_by(2) {
                    let (dag_a, dag_b) = array_mut_ref!(&mut dags, [i, target]);
                    dag_a.merge(dag_b);
                    dag_a.push(1);
                }
            } else {
                for target in (1..dags.len() - 2).step_by(2) {
                    let (dag_a, dag_b) = array_mut_ref!(&mut dags, [i, target]);
                    dag_a.merge(dag_b);
                    dag_a.push(1);
                }
            }
        }

        let len = dags.len();
        let (dag_a, dag_b) = array_mut_ref!(&mut dags, [len - 2, len - 1]);
        dag_a.push(1);
        dag_b.push(1);
        dag_a.merge(dag_b);
        let a = dag_a.get_last_node().id;
        let b = dag_b.get_last_node().id;
        let mut actual = dag_a.find_common_ancestor(&[a], &[b]);
        actual.sort();
        let actual = actual.iter().copied().collect::<Vec<_>>();
        if actual != expected {
            println!("{}", dag_a.mermaid());
        }

        prop_assert_eq!(actual, expected);
        Ok(())
    }
}

#[cfg(test)]
mod dag_partial_iter {
    use loro_common::HasCounterSpan;

    use crate::{dag::iter::IterReturn, tests::PROPTEST_FACTOR_10};

    use super::*;

    fn test_partial_iter(
        dag_num: i32,
        mut interactions: Vec<Interaction>,
    ) -> Result<(), TestCaseError> {
        preprocess(&mut interactions, dag_num);
        let mut dags = Vec::new();
        for i in 0..dag_num {
            dags.push(TestDag::new(i as PeerID));
        }

        for interaction in interactions.iter_mut() {
            interaction.apply(&mut dags);
        }

        for i in 1..dag_num {
            let (a, b) = array_mut_ref!(&mut dags, [0, i as usize]);
            a.merge(b);
        }

        let a = &dags[0];
        let mut nodes = Vec::new();
        for (node, vv) in a.iter_with_vv() {
            nodes.push((node, vv));
        }

        let mut map = FxHashMap::default();
        for (node, vv) in nodes.iter() {
            map.insert(node.id, vv.clone());
        }
        let vec: Vec<_> = nodes.iter().enumerate().collect();
        {
            // println!("{}", a.mermaid());
        }
        for &(i, (node, vv)) in vec.iter() {
            if i > 3 {
                break;
            }

            for &(j, (_other_node, other_vv)) in vec.iter() {
                if i >= j {
                    continue;
                }

                let diff_spans = other_vv.diff(vv).left;
                {
                    // println!("TARGET IS TO GO FROM {} TO {}", node.id, other_node.id);
                    // dbg!(&other_vv, &vv, &diff_spans);
                }
                let mut target_vv = vv.clone();
                target_vv.forward(&diff_spans);
                let mut vv = vv.clone();

                for IterReturn {
                    data,
                    forward,
                    retreat,
                    slice,
                } in a.iter_causal(&[node.id], diff_spans.clone())
                {
                    let sliced = data.slice(slice.start as usize, slice.end as usize);
                    {
                        // println!("-----------------------------------");
                        // dbg!(&sliced, &forward, &retreat, slice);
                    }
                    assert!(diff_spans
                        .get(&data.id.peer)
                        .unwrap()
                        .contains(sliced.id.counter));
                    vv.forward(&forward);
                    vv.retreat(&retreat);
                    let mut data_vv = map.get(&data.id).unwrap().clone();
                    data_vv.extend_to_include(IdSpan::new(
                        sliced.id.peer,
                        sliced.id.counter,
                        sliced.id.counter + 1,
                    ));
                    data_vv.shrink_to_exclude(IdSpan::new(
                        sliced.id.peer,
                        sliced.id.counter,
                        sliced.ctr_end(),
                    ));
                    assert_eq!(vv, data_vv, "{} {}", data.id, sliced.id);
                }
            }
        }

        Ok(())
    }

    #[test]
    #[ignore]
    fn issue() {
        if let Err(err) = test_partial_iter(
            5,
            vec![
                Interaction {
                    dag_idx: 0,
                    merge_with: None,
                    len: 2,
                },
                Interaction {
                    dag_idx: 1,
                    merge_with: Some(0),
                    len: 1,
                },
                Interaction {
                    dag_idx: 0,
                    merge_with: None,
                    len: 1,
                },
                Interaction {
                    dag_idx: 0,
                    merge_with: Some(1),
                    len: 1,
                },
            ],
        ) {
            panic!("{}", err);
        }
    }

    proptest! {
        #[test]
        #[ignore]
        fn proptest_iter_2(
            interactions in prop::collection::vec(gen_interaction(2), 0..40 * PROPTEST_FACTOR_10),
        ) {
            test_partial_iter(2, interactions)?;
        }

        fn proptest_iter_3(
            interactions in prop::collection::vec(gen_interaction(3), 0..40 * PROPTEST_FACTOR_10),
        ) {
            test_partial_iter(3, interactions)?;
        }

        fn proptest_iter_5(
            interactions in prop::collection::vec(gen_interaction(5), 0..40 * PROPTEST_FACTOR_10),
        ) {
            test_partial_iter(5, interactions)?;
        }
    }
}
