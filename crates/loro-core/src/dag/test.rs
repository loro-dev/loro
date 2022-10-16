#![cfg(test)]

use super::*;
use crate::{
    array_mut_ref,
    change::Lamport,
    id::{ClientID, Counter, ID},
    span::IdSpan,
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
    fn id_start(&self) -> ID {
        self.id
    }
    fn lamport_start(&self) -> Lamport {
        self.lamport
    }
    fn len(&self) -> usize {
        self.len
    }
    fn deps(&self) -> &[ID] {
        &self.deps
    }
}

#[derive(Debug, PartialEq, Eq)]
struct TestDag {
    nodes: FxHashMap<ClientID, Vec<TestNode>>,
    frontier: Vec<ID>,
    version_vec: VersionVector,
    next_lamport: Lamport,
    client_id: ClientID,
}

impl TestDag {
    fn is_first(&self) -> bool {
        *self.version_vec.get(&self.client_id).unwrap_or(&0) == 0
    }
}

impl Dag for TestDag {
    type Node = TestNode;

    fn get(&self, id: ID) -> Option<&Self::Node> {
        self.nodes.get(&id.client_id)?.iter().find(|node| {
            id.counter >= node.id.counter && id.counter < node.id.counter + node.len as Counter
        })
    }

    fn frontier(&self) -> &[ID] {
        &self.frontier
    }

    fn roots(&self) -> Vec<&Self::Node> {
        self.nodes.values().map(|v| &v[0]).collect()
    }

    fn contains(&self, id: ID) -> bool {
        self.version_vec
            .get(&id.client_id)
            .and_then(|x| if *x > id.counter { Some(()) } else { None })
            .is_some()
    }

    fn vv(&self) -> VersionVector {
        self.version_vec.clone()
    }
}

impl TestDag {
    pub fn new(client_id: ClientID) -> Self {
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
        let deps = std::mem::replace(&mut self.frontier, vec![id]);
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

    fn _try_push_node(
        &mut self,
        node: &TestNode,
        pending: &mut Vec<(u64, usize)>,
        i: usize,
    ) -> bool {
        let client_id = node.id.client_id;
        if self.contains(node.id) {
            return false;
        }
        if node.deps.iter().any(|dep| !self.contains(*dep)) {
            pending.push((client_id, i));
            return true;
        }
        update_frontier(
            &mut self.frontier,
            node.id.inc((node.len() - 1) as Counter),
            &node.deps,
        );
        self.nodes.entry(client_id).or_default().push(node.clone());
        self.version_vec
            .insert(client_id, node.id.counter + node.len as Counter);
        self.next_lamport = self.next_lamport.max(node.lamport + node.len as u32);
        false
    }
}

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
            client_id: 0,
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
    assert_eq!(
        b.find_common_ancestor(ID::new(0, 2), ID::new(1, 1))
            .first()
            .copied(),
        Some(ID::new(1, 0))
    );
}

#[derive(Debug, Clone, Copy)]
struct Interaction {
    dag_idx: usize,
    merge_with: Option<usize>,
    len: usize,
}

impl Interaction {
    fn gen(rng: &mut impl rand::Rng, num: usize) -> Self {
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

mod mermaid {

    use super::*;

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
        a.merge(&c);
        a.push(2);
        b.merge(&a);
        println!("{}", b.mermaid());
    }

    #[test]
    fn gen() {
        let num = 5;
        let mut rng = rand::thread_rng();
        let mut dags = (0..num).map(TestDag::new).collect::<Vec<_>>();
        for _ in 0..100 {
            Interaction::gen(&mut rng, num as usize).apply(&mut dags);
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

mod find_path {
    use super::*;

    #[test]
    fn no_path() {
        let mut a = TestDag::new(0);
        let mut b = TestDag::new(1);
        a.push(1);
        b.push(1);
        a.merge(&b);
        let actual = a.find_path(ID::new(0, 0), ID::new(1, 0));
        assert_eq!(actual, None);
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
        let actual = a.find_path(ID::new(0, 1), ID::new(1, 0));
        assert_eq!(
            actual,
            Some(Path {
                retreat: vec![IdSpan::new(0, 1, 0)],
                forward: vec![IdSpan::new(1, 0, 1)],
            })
        );
    }

    #[test]
    fn middle() {
        let mut a = TestDag::new(0);
        let mut b = TestDag::new(1);
        a.push(4);
        b.push(1);
        b.push(1);
        let node = b.get_last_node();
        node.deps.push(ID::new(0, 2));
        b.merge(&a);
        let actual = b.find_path(ID::new(0, 3), ID::new(1, 1));
        assert_eq!(
            actual,
            Some(Path {
                retreat: vec![IdSpan::new(0, 3, 2)],
                forward: vec![IdSpan::new(1, 1, 2)],
            })
        );
    }
}

mod find_common_ancestors {
    use super::*;

    #[test]
    fn no_common_ancestors() {
        let mut a = TestDag::new(0);
        let mut b = TestDag::new(1);
        a.push(1);
        b.push(1);
        a.merge(&b);
        let actual = a
            .find_common_ancestor(ID::new(0, 0), ID::new(1, 0))
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
            .find_common_ancestor(ID::new(0, 0), ID::new(1, 0))
            .first()
            .copied();
        assert_eq!(actual, None);
    }

    #[test]
    fn dep_in_middle() {
        let mut a = TestDag::new(0);
        let mut b = TestDag::new(1);
        a.push(4);
        b.push(4);
        b.push(5);
        b.merge(&a);
        b.frontier.retain(|x| x.client_id == 1);
        let k = b.nodes.get_mut(&1).unwrap();
        k[1].deps.push(ID::new(0, 2));
        assert_eq!(
            b.find_common_ancestor(ID::new(0, 3), ID::new(1, 8))
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
        a1.push(3);
        a2.push(2);
        a2.merge(&a0);
        a2.push(1);
        a1.merge(&a2);
        a2.push(1);
        a1.push(1);
        a1.merge(&a2);
        a1.push(1);
        a1.nodes
            .get_mut(&1)
            .unwrap()
            .last_mut()
            .unwrap()
            .deps
            .push(ID::new(0, 1));
        a0.push(1);
        a1.merge(&a2);
        a1.merge(&a0);
        assert_eq!(
            a1.find_common_ancestor(ID::new(0, 3), ID::new(1, 4))
                .first()
                .copied(),
            Some(ID::new(0, 2))
        );
    }
}

#[cfg(not(no_proptest))]
mod find_common_ancestors_proptest {
    use proptest::prelude::*;

    use crate::{array_mut_ref, unsafe_array_mut_ref};

    use super::*;

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

    proptest! {
        #[test]
        fn test_2dags(
            before_merged_insertions in prop::collection::vec(gen_interaction(2), 0..300),
            after_merged_insertions in prop::collection::vec(gen_interaction(2), 0..300)
        ) {
            test_single_common_ancestor(2, before_merged_insertions, after_merged_insertions)?;
        }

        #[test]
        fn test_4dags(
            before_merged_insertions in prop::collection::vec(gen_interaction(4), 0..300),
            after_merged_insertions in prop::collection::vec(gen_interaction(4), 0..300)
        ) {
            test_single_common_ancestor(4, before_merged_insertions, after_merged_insertions)?;
        }

        #[test]
        fn test_10dags(
            before_merged_insertions in prop::collection::vec(gen_interaction(10), 0..300),
            after_merged_insertions in prop::collection::vec(gen_interaction(10), 0..300)
        ) {
            test_single_common_ancestor(10, before_merged_insertions, after_merged_insertions)?;
        }

        #[test]
        fn test_mul_ancestors_8dags(
            before_merged_insertions in prop::collection::vec(gen_interaction(10), 0..300),
            after_merged_insertions in prop::collection::vec(gen_interaction(10), 0..300)
        ) {
            test_mul_ancestors::<3>(10, before_merged_insertions, after_merged_insertions)?;
        }
    }

    #[test]
    fn issue() {
        if let Err(err) = test_mul_ancestors::<3>(
            10,
            vec![],
            vec![Interaction {
                dag_idx: 4,
                merge_with: Some(2),
                len: 1,
            }],
        ) {
            println!("{}", err);
            panic!();
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

    fn test_single_common_ancestor(
        dag_num: i32,
        mut before_merge_insertion: Vec<Interaction>,
        mut after_merge_insertion: Vec<Interaction>,
    ) -> Result<(), TestCaseError> {
        preprocess(&mut before_merge_insertion, dag_num);
        preprocess(&mut after_merge_insertion, dag_num);
        let mut dags = Vec::new();
        for i in 0..dag_num {
            dags.push(TestDag::new(i as ClientID));
        }

        for interaction in before_merge_insertion {
            apply(interaction, &mut dags);
        }

        let (dag0,): (&mut TestDag,) = unsafe_array_mut_ref!(&mut dags, [0]);
        for dag in &dags[1..] {
            dag0.merge(dag);
        }

        dag0.push(1);
        let expected = dag0.frontier()[0];
        for dag in &mut dags[1..] {
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
        let a = dags[0].nodes.get(&0).unwrap().last().unwrap().id;
        let b = dags[1].nodes.get(&1).unwrap().last().unwrap().id;
        let actual = dags[0].find_common_ancestor(a, b);
        prop_assert_eq!(actual.first().copied().unwrap(), expected);
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
            dags.push(TestDag::new(i as ClientID));
        }

        for interaction in before_merge_insertion {
            apply(interaction, &mut dags);
        }

        for target in 0..N {
            for i in N..dags.len() {
                let (target, dag): (&mut TestDag, &mut TestDag) =
                    unsafe_array_mut_ref!(dags, [target, i]);
                dag.merge(target);
                target.merge(dag);
            }
        }

        let mut expected = Vec::with_capacity(N);
        for i in 0..N {
            dags[i].push(1);
            expected.push(dags[i].frontier[0]);
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

            let (dag,): (&mut TestDag,) = unsafe_array_mut_ref!(&mut dags, [interaction.dag_idx]);
            if dag.is_first() {
                // need to merge to one of the common ancestors first
                let target = interaction.dag_idx % N;
                dag.merge(&dags[target]);
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
        let mut actual = dag_a.find_common_ancestor(a, b);
        actual.sort();
        let actual = actual.iter().copied().collect::<Vec<_>>();
        if actual != expected {
            println!("{}", dag_to_mermaid(dag_a));
        }

        prop_assert_eq!(actual, expected);
        Ok(())
    }
}
