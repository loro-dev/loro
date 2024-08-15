use super::DagUtils;
use crate::version::Frontiers;
use num::Zero;
use std::{collections::BTreeMap, ops::Range};
use tracing::trace;

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

pub(crate) fn iter_dag_with_vv<T, D: Dag<Node = T>>(dag: &D) -> DagIteratorVV<'_, T> {
    DagIteratorVV {
        dag,
        vv_map: Default::default(),
        heap: BinaryHeap::new(),
    }
}

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
impl<'a, T: DagNode> Iterator for DagIterator<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.visited.is_empty() {
            if self.dag.vv().len() == 0 {
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
impl<'a, T: DagNode> Iterator for DagIteratorVV<'a, T> {
    type Item = (&'a T, VersionVector);

    fn next(&mut self) -> Option<Self::Item> {
        if self.vv_map.is_empty() {
            if self.dag.vv().len() == 0 {
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
                for &dep_id in node.deps() {
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
pub(crate) struct IterReturn<'a, T> {
    #[allow(unused)]
    pub retreat: IdSpanVector,
    #[allow(unused)]
    pub forward: IdSpanVector,
    /// data is a reference, it need to be sliced by the counter_range to get the underlying data
    pub data: &'a T,
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
                                succ.entry(*dep).or_default().push(id);
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

        trace!("out_degrees={:?}", &out_degrees);
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

impl<'a, T: DagNode + 'a, D: Dag<Node = T>> Iterator for DagCausalIter<'a, D> {
    type Item = IterReturn<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
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
        let target_span = self.target.get_mut(&node_id.peer).unwrap();
        debug_assert_eq!(
            node_id.counter,
            target_span.min(),
            "{} {:?}",
            node_id,
            target_span
        );

        // // node_id may points into the middle of the node, we need to slice
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
        assert!(slice_end > slice_from);

        let last_counter = node.id_last().counter;
        target_span.set_start(last_counter + 1);

        let deps: SmallVec<[_; 2]> = if slice_from == 0 {
            node.deps().iter().copied().collect()
        } else {
            smallvec::smallvec![node.id_start().inc(slice_from - 1)]
        };

        let path = self.dag.find_path(&self.frontier, &deps);

        // tracing::span!(tracing::Level::INFO, "Dag Causal");

        //

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

        Some(IterReturn {
            retreat: path.left,
            forward: path.right,
            data: node,
            slice: slice_from..slice_end,
        })
    }
}
