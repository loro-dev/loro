use super::DagUtils;
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
        Some(self.lamport.cmp(&other.lamport).reverse())
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
                let mut vv = None;
                for &dep_id in node.deps() {
                    let dep = self.dag.get(dep_id).unwrap();
                    let dep_vv = self.vv_map.get(&dep.id_start()).unwrap();
                    if vv.is_none() {
                        vv = Some(dep_vv.clone());
                    } else {
                        vv.as_mut().unwrap().merge(dep_vv);
                    }

                    if dep.id_start() != dep_id {
                        vv.as_mut().unwrap().set_last(dep_id);
                    }
                }

                vv.unwrap_or_else(VersionVector::new)
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
    in_degrees: FxHashMap<ID, usize>,
    succ: BTreeMap<ID, Frontiers>,
    stack: Vec<ID>,
}

#[derive(Debug)]
pub(crate) struct IterReturn<'a, T> {
    pub retreat: IdSpanVector,
    pub forward: IdSpanVector,
    /// data is a reference, it need to be sliced by the counter_range to get the underlying data
    pub data: &'a T,
    /// data[slice] is the data we want to return
    pub slice: Range<i32>,
}

impl<'a, T: DagNode, D: Dag<Node = T>> DagCausalIter<'a, D> {
    pub fn new(dag: &'a D, from: Frontiers, target: IdSpanVector) -> Self {
        // debug_dbg!(&from, &target);
        let mut in_degrees: FxHashMap<ID, usize> = FxHashMap::default();
        let mut succ: BTreeMap<ID, Frontiers> = BTreeMap::default();
        let mut stack = Vec::new();
        let mut q = vec![];
        for id in target.iter() {
            if id.1.content_len() > 0 {
                let id = id.id_start();
                q.push(id);
            }
        }

        // traverse all nodes
        while let Some(id) = q.pop() {
            let client = id.peer;
            let node = dag.get(id).unwrap();
            let deps = node.deps();
            if deps.len().is_zero() {
                in_degrees.insert(id, 0);
            }
            for dep in deps.iter() {
                let filter = if let Some(span) = target.get(&dep.peer) {
                    dep.counter < span.min()
                } else {
                    true
                };
                if filter {
                    in_degrees.entry(id).or_insert(0);
                } else {
                    in_degrees.entry(id).and_modify(|i| *i += 1).or_insert(1);
                }
                succ.entry(*dep).or_default().push(id);
            }
            let mut target_span = *target.get(&client).unwrap();
            let last_counter = node.id_last().counter;
            target_span.set_start(last_counter + 1);
            if target_span.content_len() > 0 {
                q.push(ID::new(client, last_counter + 1))
            }
        }

        in_degrees.retain(|k, v| {
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
            in_degrees,
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
        debug_log::group!("Dag Causal");
        debug_log::debug_dbg!(&deps);
        debug_log::debug_dbg!(&path);
        debug_log::group_end!();
        // NOTE: we expect user to update the tracker, to apply node, after visiting the node
        self.frontier = Frontiers::from_id(node.id_start().inc(slice_end - 1));

        let current_client = node_id.peer;
        let mut keys = Vec::new();
        let mut heap = BinaryHeap::new();
        // The in-degree of the successor node minus 1, and if it becomes 0, it is added to the heap
        for (key, succ) in self.succ.range(node.id_start()..node.id_end()) {
            keys.push(*key);
            for succ_id in succ.iter() {
                self.in_degrees.entry(*succ_id).and_modify(|i| *i -= 1);
                if let Some(in_degree) = self.in_degrees.get(succ_id) {
                    if in_degree.is_zero() {
                        heap.push((succ_id.peer != current_client, *succ_id));
                        self.in_degrees.remove(succ_id);
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

#[cfg(test)]
mod test {
    use crate::{
        configure::Configure,
        dag::DagUtils,
        id::{Counter, ID},
        LoroCore, VersionVector,
    };
    #[test]
    fn my_case() {
        let mut loro_a = LoroCore::new(Default::default(), Some(1));
        let mut loro_b = LoroCore::new(Default::default(), Some(2));
        let mut loro_c = LoroCore::new(Default::default(), Some(3));

        let mut text_a = loro_a.get_text("text");
        let mut text_b = loro_b.get_text("text");
        let mut text_c = loro_c.get_text("text");

        text_a.insert(&loro_a, 0, "a1").unwrap();
        text_a.insert(&loro_a, 0, "a2").unwrap();
        text_a.insert(&loro_a, 0, "a3").unwrap();

        text_b.insert(&loro_b, 0, "b1").unwrap();

        loro_c.decode(&loro_b.encode_all()).unwrap();

        text_c.insert(&loro_c, 0, "c1").unwrap();
        let from: Vec<ID> = {
            let m = loro_c.log_store.try_read().unwrap();
            let f = m.frontiers();
            f.to_vec()
        };
        let from_vv = loro_c.vv_cloned();
        println!("c start vv: {:?}", loro_c.vv_cloned());

        text_b.insert(&loro_b, 0, "b2").unwrap();
        text_b.insert(&loro_b, 0, "b3").unwrap();

        loro_b
            .decode(&loro_a.encode_from(loro_b.vv_cloned()))
            .unwrap();

        text_b.insert(&loro_b, 0, "b4").unwrap();

        text_c.insert(&loro_c, 0, "c2").unwrap();

        loro_c
            .decode(&loro_b.encode_from(loro_c.vv_cloned()))
            .unwrap();

        text_c.insert(&loro_c, 0, "c3").unwrap();
        let mut vv = from_vv.clone();

        println!(
            "from {:?}, diff {:?}\n#################",
            &from,
            loro_c.vv_cloned().diff(&from_vv).left
        );

        let store_c = loro_c.log_store.try_read().unwrap();

        for n in store_c.iter_causal(&from, loro_c.vv_cloned().diff(&from_vv).left) {
            // println!("retreat {:?} forward {:?}", &n.retreat, &n.forward);
            // println!("data: {:?}", store_c.change_to_export_format(n.data));
            vv.retreat(&n.retreat);
            vv.forward(&n.forward);
            let end = n.slice.end;
            let change = n.data;

            vv.set_end(ID::new(change.id.peer, end as Counter + change.id.counter));
            println!("{:?}\n", vv);
        }
    }

    #[test]
    fn parallel_case() {
        let mut c1 = LoroCore::new(
            Configure {
                ..Default::default()
            },
            Some(1),
        );
        let mut c2 = LoroCore::new(
            Configure {
                ..Default::default()
            },
            Some(2),
        );
        let mut text1 = c1.get_text("text");
        let mut text2 = c2.get_text("text");
        for _ in 0..5 {
            text1.insert(&c1, 0, "1").unwrap();
            text2.insert(&c2, 0, "2").unwrap();
        }
        c1.decode(&c2.encode_from(c1.vv_cloned())).unwrap();

        let mut from_vv = VersionVector::new();
        from_vv.set_last(ID {
            peer: 1,
            counter: 1,
        });
        from_vv.set_last(ID {
            peer: 2,
            counter: 1,
        });
        let mut vv = from_vv.clone();
        let c1_store = c1.log_store.try_read().unwrap();
        for n in c1_store.iter_causal(
            &[
                ID {
                    peer: 1,
                    counter: 1,
                },
                ID {
                    peer: 2,
                    counter: 1,
                },
            ],
            c1.vv_cloned().diff(&from_vv).left,
        ) {
            println!("retreat {:?} forward {:?}", &n.retreat, &n.forward);
            // println!("data: {:?}", store_c.change_to_export_format(n.data));
            vv.retreat(&n.retreat);
            vv.forward(&n.forward);
            let end = n.slice.end;
            let change = n.data;

            vv.set_end(ID::new(change.id.peer, end as Counter + change.id.counter));
            println!("{:?}\n", vv);
        }
    }
}
