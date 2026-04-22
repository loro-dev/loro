use crate::version::Frontiers;
use num::Zero;
use std::{collections::BTreeMap, ops::Range};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct IdHeapItem {
    id: ID,
    lamport: Lamport,
}

impl PartialOrd for IdHeapItem {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for IdHeapItem {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.lamport.cmp(&other.lamport).reverse()
    }
}

#[allow(dead_code)]
pub(crate) fn iter_dag_with_vv<T, D: Dag<Node = T>>(dag: &D) -> DagIteratorVV<'_, T> {
    DagIteratorVV {
        dag,
        vv_map: Default::default(),
        heap: BinaryHeap::new(),
    }
}

#[allow(dead_code)]
pub(crate) fn iter_dag<T>(dag: &dyn Dag<Node = T>) -> DagIterator<'_, T> {
    DagIterator {
        dag,
        visited: VersionVector::new(),
        heap: BinaryHeap::new(),
    }
}

pub struct DagIterator<'a, T> {
    dag: &'a dyn Dag<Node = T>,
    /// Because all deps' lamports are smaller than current node's lamport.
    /// We can use the lamport to sort the nodes so that each node's deps are processed before itself.
    ///
    /// The ids in this heap are start ids of nodes. It won't be a id pointing to the middle of a node.
    heap: BinaryHeap<IdHeapItem>,
    visited: VersionVector,
}

/// Should only use it on debug, because it's slow and likely to use lots of mem
impl<T: DagNode> Iterator for DagIterator<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.visited.is_empty() {
            if self.dag.vv().is_empty() {
                return None;
            }

            for (&client_id, _) in self.dag.vv().iter() {
                if let Some(node) = self.dag.get(ID::new(client_id, 0)) {
                    self.heap.push(IdHeapItem {
                        id: ID::new(client_id, 0),
                        lamport: node.lamport(),
                    });
                }

                self.visited.insert(client_id, 0);
            }
        }

        if !self.heap.is_empty() {
            let item = self.heap.pop().unwrap();
            let id = item.id;
            let node = self.dag.get(id).unwrap();
            // push next node from the same client to the heap
            let next_id = id.inc(node.content_len() as i32);
            if self.dag.contains(next_id) {
                let next_node = self.dag.get(next_id).unwrap();
                self.heap.push(IdHeapItem {
                    id: next_id,
                    lamport: next_node.lamport(),
                });
            }

            return Some(node);
        }

        None
    }
}

pub(crate) struct DagIteratorVV<'a, T> {
    dag: &'a dyn Dag<Node = T>,
    /// we should keep every nodes starting id inside this map
    vv_map: FxHashMap<ID, VersionVector>,
    /// Because all deps' lamports are smaller than current node's lamport.
    /// We can use the lamport to sort the nodes so that each node's deps are processed before itself.
    ///
    /// The ids in this heap are start ids of nodes. It won't be a id pointing to the middle of a node.
    heap: BinaryHeap<IdHeapItem>,
}

/// Should only use it on debug, because it's slow and likely to use lots of mem
impl<T: DagNode> Iterator for DagIteratorVV<'_, T> {
    type Item = (T, VersionVector);

    fn next(&mut self) -> Option<Self::Item> {
        if self.vv_map.is_empty() {
            if self.dag.vv().is_empty() {
                return None;
            }

            for (&client_id, _) in self.dag.vv().iter() {
                let vv = VersionVector::new();
                if let Some(node) = self.dag.get(ID::new(client_id, 0)) {
                    if node.lamport() == 0 {
                        self.vv_map.insert(ID::new(client_id, 0), vv.clone());
                    }

                    self.heap.push(IdHeapItem {
                        id: ID::new(client_id, 0),
                        lamport: node.lamport(),
                    });
                }
            }
        }

        if !self.heap.is_empty() {
            let item = self.heap.pop().unwrap();
            let id = item.id;
            let node = self.dag.get(id).unwrap();
            debug_assert_eq!(id, node.id_start());
            let mut vv = {
                // calculate vv
                let mut vv: Option<VersionVector> = None;
                for dep_id in node.deps().iter() {
                    let dep = self.dag.get(dep_id).unwrap();
                    let dep_vv = self.vv_map.get(&dep.id_start()).unwrap();
                    if let Some(vv) = vv.as_mut() {
                        vv.merge(dep_vv);
                    } else {
                        vv = Some(dep_vv.clone());
                    }

                    if dep.id_start() != dep_id {
                        vv.as_mut().unwrap().set_last(dep_id);
                    }
                }

                vv.unwrap_or_default()
            };

            vv.try_update_last(id);
            self.vv_map.insert(id, vv.clone());

            // push next node from the same client to the heap
            let next_id = id.inc(node.content_len() as i32);
            if self.dag.contains(next_id) {
                let next_node = self.dag.get(next_id).unwrap();
                self.heap.push(IdHeapItem {
                    id: next_id,
                    lamport: next_node.lamport(),
                });
            }

            return Some((node, vv));
        }

        None
    }
}

/// Visit every span in the target IdSpanVector.
/// It's guaranteed that the spans are visited in causal order, and each span is visited only once.
/// When visiting a span, we will checkout to the version where the span was created
pub(crate) struct DagCausalIter<'a, Dag> {
    dag: &'a Dag,
    frontier: Frontiers,
    target: IdSpanVector,
    /// how many dependencies are inside target for each id
    out_degrees: FxHashMap<ID, usize>,
    succ: BTreeMap<ID, Vec<ID>>,
    stack: Vec<ID>,
}

#[derive(Debug)]
pub(crate) struct IterReturn<T> {
    /// data is a reference, it need to be sliced by the counter_range to get the underlying data
    pub data: T,
    /// data[slice] is the data we want to return
    #[allow(unused)]
    pub slice: Range<i32>,
}

impl<'a, T: DagNode, D: Dag<Node = T> + Debug> DagCausalIter<'a, D> {
    pub fn new(dag: &'a D, from: Frontiers, target: IdSpanVector) -> Self {
        let mut out_degrees: FxHashMap<ID, usize> = FxHashMap::default();
        let mut succ: BTreeMap<ID, Vec<ID>> = BTreeMap::default();
        let mut stack = Vec::new();
        let mut q = vec![];
        for id in target.iter() {
            if id.1.content_len() > 0 {
                let id = id.id_start();
                q.push(id);
            }
        }

        // traverse all nodes, calculate the out_degrees
        // if out_degree is 0, then it can be iterated directly
        while let Some(id) = q.pop() {
            let client = id.peer;
            let node = dag.get(id).unwrap();
            let deps = node.deps();
            out_degrees.insert(
                id,
                deps.iter()
                    .filter(|&dep| {
                        if let Some(span) = target.get(&dep.peer) {
                            let included_in_target =
                                dep.counter >= span.min() && dep.counter <= span.max();
                            if included_in_target {
                                succ.entry(dep).or_default().push(id);
                            }
                            included_in_target
                        } else {
                            false
                        }
                    })
                    .count(),
            );
            let target_span = target.get(&client).unwrap();
            let last_counter = node.id_last().counter;
            if target_span.max() > last_counter {
                q.push(ID::new(client, last_counter + 1))
            }
        }

        // Enforce per-peer counter order. Within a single peer, the iterator
        // must visit tracked node-starts in ascending counter order even when
        // the nodes are concurrent (i.e. don't declare each other as deps).
        // Without this, two same-peer nodes can both have zero in-degree and
        // end up on the stack at once; the LIFO pop order would then violate
        // the iterator's invariant that each popped id equals the current
        // target_span.min() for its peer.
        let mut by_peer: FxHashMap<PeerID, Vec<Counter>> = FxHashMap::default();
        for id in out_degrees.keys() {
            by_peer.entry(id.peer).or_default().push(id.counter);
        }
        for (peer, mut counters) in by_peer {
            if counters.len() < 2 {
                continue;
            }
            counters.sort_unstable();
            for pair in counters.windows(2) {
                let prev = ID::new(peer, pair[0]);
                let curr = ID::new(peer, pair[1]);
                if let Some(deg) = out_degrees.get_mut(&curr) {
                    *deg += 1;
                }
                succ.entry(prev).or_default().push(curr);
            }
        }

        out_degrees.retain(|k, v| {
            if v.is_zero() {
                stack.push(*k);
                return false;
            }
            true
        });

        Self {
            dag,
            frontier: from,
            target,
            out_degrees,
            succ,
            stack,
        }
    }
}

impl<T: DagNode, D: Dag<Node = T>> Iterator for DagCausalIter<'_, D> {
    type Item = IterReturn<T>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.stack.is_empty() {
                debug_assert_eq!(
                    0,
                    self.target
                        .iter()
                        .map(|x| x.1.content_len() as i32)
                        .sum::<i32>()
                );
                return None;
            }

            let node_id = self.stack.pop().unwrap();
            let (node, slice_from, slice_end, next_same_peer) = {
                let target_span = self.target.get_mut(&node_id.peer).unwrap();
                if node_id.counter != target_span.min() {
                    debug_assert!(
                        node_id.counter < target_span.min(),
                        "{} {:?}",
                        node_id,
                        target_span
                    );
                    continue;
                }

                // node_id may point into the middle of the node, we need to slice
                let node = self.dag.get(node_id).unwrap();
                // node start_id may be smaller than node_id
                let counter = node.id_span().counter;
                let slice_from = if counter.start < target_span.start {
                    target_span.start - counter.start
                } else {
                    0
                };
                let slice_end = if counter.end < target_span.end {
                    counter.end - counter.start
                } else {
                    target_span.end - counter.start
                };

                if slice_end <= slice_from {
                    debug_assert_eq!(slice_end, slice_from, "{node_id} {:?}", target_span);
                    continue;
                }

                let consumed_last_counter = counter.start + slice_end - 1;
                target_span.set_start(consumed_last_counter + 1);
                let next_same_peer = if target_span.content_len() > 0 {
                    Some(ID::new(node_id.peer, target_span.min()))
                } else {
                    None
                };

                (node, slice_from, slice_end, next_same_peer)
            };

            if let Some(next_id) = next_same_peer {
                if !self.out_degrees.contains_key(&next_id) && !self.stack.contains(&next_id) {
                    self.stack.push(next_id);
                }
            }

            // NOTE: we expect user to update the tracker, to apply node, after visiting the node
            self.frontier = Frontiers::from_id(node.id_start().inc(slice_end - 1));

            let current_peer = node_id.peer;
            let mut keys = Vec::new();
            let mut heap = BinaryHeap::new();
            // The in-degree of the successor node minus 1, and if it becomes 0, it is added to the heap
            for (key, succ) in self.succ.range(node.id_start()..node.id_end()) {
                keys.push(*key);
                for succ_id in succ.iter() {
                    self.out_degrees.entry(*succ_id).and_modify(|i| *i -= 1);
                    if let Some(in_degree) = self.out_degrees.get(succ_id) {
                        if in_degree.is_zero() {
                            heap.push((succ_id.peer != current_peer, *succ_id));
                            self.out_degrees.remove(succ_id);
                        }
                    }
                }
            }
            // Nodes that have been traversed are removed from the graph to avoid being covered by other node ranges again
            keys.into_iter().for_each(|k| {
                self.succ.remove(&k);
            });
            while let Some(id) = heap.pop() {
                self.stack.push(id.1)
            }

            return Some(IterReturn {
                data: node,
                slice: slice_from..slice_end,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        change::Lamport,
        id::ID,
        span::{CounterSpan, HasId, HasLamport},
        version::VersionVector,
    };
    use rle::{HasLength, Sliceable};

    #[derive(Debug, Clone)]
    struct DummyNode {
        id: ID,
        lamport: Lamport,
        len: usize,
        deps: Frontiers,
    }

    impl DagNode for DummyNode {
        fn deps(&self) -> &Frontiers {
            &self.deps
        }
    }

    impl Sliceable for DummyNode {
        fn slice(&self, from: usize, to: usize) -> Self {
            Self {
                id: self.id.inc(from as i32),
                lamport: self.lamport + from as Lamport,
                len: to - from,
                deps: if from > 0 {
                    self.id.inc(from as i32 - 1).into()
                } else {
                    self.deps.clone()
                },
            }
        }
    }

    impl HasLamport for DummyNode {
        fn lamport(&self) -> Lamport {
            self.lamport
        }
    }

    impl HasId for DummyNode {
        fn id_start(&self) -> ID {
            self.id
        }
    }

    impl HasLength for DummyNode {
        fn content_len(&self) -> usize {
            self.len
        }
    }

    #[derive(Debug)]
    struct DummyDag {
        nodes: BTreeMap<ID, DummyNode>,
        vv: VersionVector,
        frontier: Frontiers,
    }

    impl Dag for DummyDag {
        type Node = DummyNode;

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
            self.nodes
                .range(..=id)
                .next_back()
                .is_some_and(|(_, node)| node.contains_id(id))
        }
    }

    #[test]
    fn stale_iterator_state_repairs_missing_same_peer_continuation() {
        let first = DummyNode {
            id: ID::new(1, 0),
            lamport: 0,
            len: 5,
            deps: Frontiers::default(),
        };
        let second = DummyNode {
            id: ID::new(1, 5),
            lamport: 5,
            len: 5,
            deps: ID::new(1, 4).into(),
        };
        let mut vv = VersionVector::default();
        vv.set_end(second.id_end());
        let dag = DummyDag {
            frontier: second.id_last().into(),
            nodes: [(first.id_start(), first), (second.id_start(), second)]
                .into_iter()
                .collect(),
            vv,
        };

        let mut target = IdSpanVector::default();
        target.insert(1, CounterSpan::new(0, 7));
        let mut iter = DagCausalIter {
            dag: &dag,
            frontier: Frontiers::default(),
            target,
            out_degrees: Default::default(),
            succ: BTreeMap::default(),
            // This models the stale iterator state after `new()` saw one large
            // node and queued only the first peer segment.
            stack: vec![ID::new(1, 0)],
        };

        let first = iter.next().expect("first segment should be yielded");
        assert_eq!(first.data.id_start(), ID::new(1, 0));
        assert_eq!(first.slice, 0..5);

        let second = iter.next().expect("second segment should be re-queued");
        assert_eq!(second.data.id_start(), ID::new(1, 5));
        assert_eq!(second.slice, 0..2);

        assert!(iter.next().is_none());
    }

    /// Regression: before the fix, `DagCausalIter::new` would push both
    /// `(peer=1, counter=0)` and `(peer=1, counter=5)` onto the initial
    /// stack because neither node has any *in-target* dependency. The LIFO
    /// pop would then surface `(peer=1, counter=5)` first while
    /// `target_span.min()` is still `0`, tripping the causal-order
    /// invariant (historically: "slice_end > slice_from" / "counter <
    /// target_span.min()"). With the fix the iter synthesizes a per-peer
    /// ordering edge so the second node is only released after the first is
    /// consumed.
    #[test]
    fn two_concurrent_same_peer_nodes_visit_in_counter_order() {
        let first = DummyNode {
            id: ID::new(1, 0),
            lamport: 10,
            len: 5,
            deps: Frontiers::default(),
        };
        // `second` is concurrent with `first` from peer 1's perspective:
        // its only dep lives outside the target span (peer 2), so the BFS
        // records zero in-degree and would historically race to the stack
        // alongside `first`.
        let second = DummyNode {
            id: ID::new(1, 5),
            lamport: 20,
            len: 5,
            deps: ID::new(2, 0).into(),
        };
        let side = DummyNode {
            id: ID::new(2, 0),
            lamport: 0,
            len: 1,
            deps: Frontiers::default(),
        };

        let mut vv = VersionVector::default();
        vv.set_end(first.id_end());
        vv.set_end(second.id_end());
        vv.set_end(side.id_end());

        let dag = DummyDag {
            frontier: second.id_last().into(),
            nodes: [
                (first.id_start(), first),
                (second.id_start(), second),
                (side.id_start(), side),
            ]
            .into_iter()
            .collect(),
            vv,
        };

        // Target only peer 1. `second`'s dep on (peer 2, 0) is thus
        // *outside* the target vector — the exact condition that made both
        // same-peer nodes reach the stack concurrently before the fix.
        let mut target = IdSpanVector::default();
        target.insert(1, CounterSpan::new(0, 10));

        let mut iter = DagCausalIter::new(&dag, Frontiers::default(), target);

        let a = iter.next().expect("first peer-1 segment");
        assert_eq!(a.data.id_start(), ID::new(1, 0));
        assert_eq!(a.slice, 0..5);

        let b = iter.next().expect("second peer-1 segment");
        assert_eq!(b.data.id_start(), ID::new(1, 5));
        assert_eq!(b.slice, 0..5);

        assert!(iter.next().is_none());
    }

    #[test]
    fn larger_node_only_advances_consumed_slice() {
        let node = DummyNode {
            id: ID::new(1, 0),
            lamport: 0,
            len: 10,
            deps: Frontiers::default(),
        };
        let mut vv = VersionVector::default();
        vv.set_end(node.id_end());
        let dag = DummyDag {
            frontier: node.id_last().into(),
            nodes: [(node.id_start(), node)].into_iter().collect(),
            vv,
        };

        let mut target = IdSpanVector::default();
        target.insert(1, CounterSpan::new(0, 7));
        let mut iter = DagCausalIter {
            dag: &dag,
            frontier: Frontiers::default(),
            target,
            out_degrees: Default::default(),
            succ: BTreeMap::default(),
            stack: vec![ID::new(1, 0)],
        };

        let item = iter.next().expect("target slice should be yielded");
        assert_eq!(item.slice, 0..7);
        assert_eq!(iter.target.get(&1).unwrap().content_len(), 0);
    }
}
