use super::*;
#[allow(dead_code)]
struct BreakPoints {
    break_points: FxHashMap<PeerID, FxHashSet<Counter>>,
    /// start ID to ID. The target ID may be in the middle of an op.
    ///
    /// only includes links across different clients
    links: FxHashMap<ID, Vec<ID>>,
}

#[allow(dead_code)]
struct Output {
    clients: FxHashMap<PeerID, Vec<IdSpan>>,
    /// start ID to start ID.
    ///
    /// only includes links across different clients
    links: FxHashMap<ID, Vec<ID>>,
}

#[allow(dead_code)]
fn to_str(output: Output) -> String {
    let mut s = String::new();
    let mut indent_level = 0;
    macro_rules! new_line {
        () => {
            s += "\n";
            for _ in 0..indent_level {
                s += "    ";
            }
        };
    }
    s += "flowchart RL";
    indent_level += 1;
    new_line!();
    for (client_id, spans) in output.clients.iter() {
        s += format!("subgraph peer{client_id}").as_str();
        new_line!();
        let mut is_first = true;
        for id_span in spans.iter().rev() {
            if !is_first {
                s += " --> "
            }

            is_first = false;
            s += format!(
                "{}-{}(\"c{}: [{}, {})\")",
                id_span.peer,
                id_span.counter.start,
                id_span.peer,
                id_span.counter.start,
                id_span.counter.end,
            )
            .as_str();
        }

        new_line!();
        s += "end";
        new_line!();
        new_line!();
    }

    for (id_from, id_tos) in output.links.iter() {
        for id_to in id_tos.iter() {
            s += format!(
                "{}-{} --> {}-{}",
                id_from.peer, id_from.counter, id_to.peer, id_to.counter
            )
            .as_str();
            new_line!();
        }
    }

    s
}

#[allow(dead_code)]
fn break_points_to_output(input: BreakPoints) -> Output {
    let mut output = Output {
        clients: FxHashMap::default(),
        links: FxHashMap::default(),
    };
    let breaks: FxHashMap<PeerID, Vec<Counter>> = input
        .break_points
        .into_iter()
        .map(|(client_id, set)| {
            let mut vec: Vec<Counter> = set.iter().cloned().collect();
            vec.sort();
            (client_id, vec)
        })
        .collect();
    for (client_id, break_points) in breaks.iter() {
        let mut spans = Vec::with_capacity(break_points.len());
        for (from, to) in break_points.iter().zip(break_points.iter().skip(1)) {
            spans.push(IdSpan::new(*client_id, *from, *to));
        }
        output.clients.insert(*client_id, spans);
    }

    for (id_from, id_tos) in input.links.iter() {
        for id_to in id_tos {
            let client_breaks = breaks.get(&id_to.peer).unwrap();
            match client_breaks.binary_search(&id_to.counter) {
                Ok(_) => {
                    output.links.entry(*id_from).or_default().push(*id_to);
                }
                Err(index) => {
                    output
                        .links
                        .entry(*id_from)
                        .or_default()
                        .push(ID::new(id_to.peer, client_breaks[index - 1]));
                }
            }
        }
    }
    output
}

#[allow(dead_code)]
fn get_dag_break_points<T: DagNode>(dag: &impl Dag<Node = T>) -> BreakPoints {
    let mut break_points = BreakPoints {
        break_points: FxHashMap::default(),
        links: FxHashMap::default(),
    };

    for node in dag.iter() {
        let id = node.id_start();
        let set = break_points.break_points.entry(id.peer).or_default();
        set.insert(id.counter);
        set.insert(id.counter + node.content_len() as Counter);
        for dep in node.deps().iter() {
            if dep.peer == id.peer {
                continue;
            }

            break_points
                .break_points
                .entry(dep.peer)
                .or_default()
                .insert(dep.counter);
            break_points.links.entry(id).or_default().push(dep);
        }
    }

    break_points
}

#[allow(dead_code)]
pub(crate) fn dag_to_mermaid<T: DagNode>(dag: &impl Dag<Node = T>) -> String {
    to_str(break_points_to_output(get_dag_break_points(dag)))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use loro_common::{HasId, HasIdSpan};
    use rle::{HasLength, Sliceable};

    use super::*;

    #[derive(Clone, Debug)]
    struct TestNode {
        id: ID,
        len: usize,
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
            self.len
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
            self.nodes
                .range(..=id)
                .next_back()
                .filter(|(_, node)| node.contains_id(id))
                .map(|(_, node)| node.clone())
        }

        fn frontier(&self) -> &Frontiers {
            &self.frontier
        }

        fn vv(&self) -> &VersionVector {
            &self.vv
        }

        fn contains(&self, id: ID) -> bool {
            self.get(id).is_some()
        }
    }

    fn node(
        peer: PeerID,
        counter: Counter,
        len: usize,
        lamport: Lamport,
        deps: Frontiers,
    ) -> TestNode {
        TestNode {
            id: ID::new(peer, counter),
            len,
            lamport,
            deps,
        }
    }

    #[test]
    fn break_points_to_output_splits_spans_and_retargets_middle_links_to_span_starts() {
        let mut break_points = FxHashMap::default();
        break_points.insert(1, FxHashSet::from_iter([0, 3]));
        break_points.insert(2, FxHashSet::from_iter([0, 5, 10]));
        let from = ID::new(1, 0);
        let mut links = FxHashMap::default();
        links.insert(from, vec![ID::new(2, 7)]);

        let output = break_points_to_output(BreakPoints {
            break_points,
            links,
        });

        assert_eq!(output.clients.get(&1).unwrap(), &vec![IdSpan::new(1, 0, 3)]);
        assert_eq!(
            output.clients.get(&2).unwrap(),
            &vec![IdSpan::new(2, 0, 5), IdSpan::new(2, 5, 10)]
        );
        assert_eq!(output.links.get(&from).unwrap(), &vec![ID::new(2, 5)]);
    }

    #[test]
    fn dag_to_mermaid_includes_peer_subgraphs_split_spans_and_cross_peer_edges() {
        let first = node(1, 0, 2, 0, Frontiers::default());
        let second = node(2, 0, 1, 3, ID::new(1, 1).into());
        let dag = TestDag::new(vec![first, second], ID::new(2, 0).into());

        let graph = dag_to_mermaid(&dag);

        assert!(graph.starts_with("flowchart RL"));
        assert!(graph.contains("subgraph peer1"));
        assert!(graph.contains("subgraph peer2"));
        assert!(graph.contains("1-0(\"c1: [0, 1)\")"));
        assert!(graph.contains("1-1(\"c1: [1, 2)\")"));
        assert!(graph.contains("2-0(\"c2: [0, 1)\")"));
        assert!(graph.contains("2-0 --> 1-1"));
    }

    #[test]
    fn to_str_renders_all_links_without_requiring_hash_map_order() {
        let output = Output {
            clients: FxHashMap::from_iter([
                (1, vec![IdSpan::new(1, 0, 1), IdSpan::new(1, 1, 2)]),
                (2, vec![IdSpan::new(2, 0, 1)]),
            ]),
            links: FxHashMap::from_iter([(ID::new(2, 0), vec![ID::new(1, 0), ID::new(1, 1)])]),
        };

        let graph = to_str(output);

        assert!(graph.contains("subgraph peer1"));
        assert!(graph.contains("1-1(\"c1: [1, 2)\") --> 1-0(\"c1: [0, 1)\")"));
        assert!(graph.contains("2-0 --> 1-0"));
        assert!(graph.contains("2-0 --> 1-1"));
    }
}
