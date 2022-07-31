use std::{
    collections::{BinaryHeap, HashMap, VecDeque},
    ops::Range,
};

use fxhash::FxHashMap;
#[cfg(test)]
mod test;

use crate::{
    change::Lamport,
    id::{ClientID, Counter, ID},
    span::{CounterSpan, IdSpan},
};

pub trait DagNode {
    fn dag_id_start(&self) -> ID;
    fn lamport_start(&self) -> Lamport;
    fn len(&self) -> usize;
    fn deps(&self) -> &Vec<ID>;

    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    fn dag_id_span(&self) -> IdSpan {
        let id = self.dag_id_start();
        IdSpan {
            client_id: id.client_id,
            counter: CounterSpan::new(id.counter, id.counter + self.len() as Counter),
        }
    }

    /// inclusive end
    #[inline]
    fn dag_id_end(&self) -> ID {
        let id = self.dag_id_start();
        ID {
            client_id: id.client_id,
            counter: id.counter + self.len() as Counter - 1,
        }
    }

    #[inline]
    fn get_lamport_from_counter(&self, c: Counter) -> Lamport {
        self.lamport_start() + c - self.dag_id_start().counter
    }
}

pub(crate) trait Dag {
    type Node: DagNode;

    fn get(&self, id: ID) -> Option<&Self::Node>;
    fn contains(&self, id: ID) -> bool;
    fn frontier(&self) -> &[ID];
    fn roots(&self) -> Vec<&Self::Node>;

    //
    // TODO: Maybe use Result return type
    // TODO: benchmark
    // TODO: visited
    // how to test better?
    // - converge through other nodes
    //
    /// only returns a single root.
    /// but the least common ancestor may be more than one root.
    /// But that is a rare case.
    fn find_common_ancestor(&self, a_id: ID, b_id: ID) -> Option<ID> {
        if a_id.client_id == b_id.client_id {
            if a_id.counter <= b_id.counter {
                Some(a_id)
            } else {
                Some(b_id)
            }
        } else {
            #[derive(Debug, PartialEq, Eq)]
            struct OrdId<'a> {
                id: ID,
                lamport: Lamport,
                deps: &'a [ID],
            }

            impl<'a> PartialOrd for OrdId<'a> {
                fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                    Some(self.lamport.cmp(&other.lamport))
                }
            }

            impl<'a> Ord for OrdId<'a> {
                fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                    self.lamport.cmp(&other.lamport)
                }
            }

            let mut _a_vv: HashMap<ClientID, Counter, _> = FxHashMap::default();
            let mut _b_vv: HashMap<ClientID, Counter, _> = FxHashMap::default();
            let mut _a_heap: BinaryHeap<OrdId> = BinaryHeap::new();
            let mut _b_heap: BinaryHeap<OrdId> = BinaryHeap::new();
            {
                let a = self.get(a_id).unwrap();
                let b = self.get(b_id).unwrap();
                _a_heap.push(OrdId {
                    id: a_id,
                    lamport: a.get_lamport_from_counter(a_id.counter),
                    deps: a.deps(),
                });
                _b_heap.push(OrdId {
                    id: b_id,
                    lamport: b.get_lamport_from_counter(b_id.counter),
                    deps: b.deps(),
                });
                _a_vv.insert(a_id.client_id, a_id.counter + 1);
                _b_vv.insert(b_id.client_id, b_id.counter + 1);
            }

            while !_a_heap.is_empty() || !_b_heap.is_empty() {
                let (a_heap, b_heap, a_vv, b_vv, _swapped) = if _a_heap.is_empty()
                    || (_a_heap.peek().map(|x| x.lamport).unwrap_or(0)
                        < _b_heap.peek().map(|x| x.lamport).unwrap_or(0))
                {
                    // swap
                    (&mut _b_heap, &mut _a_heap, &mut _b_vv, &mut _a_vv, true)
                } else {
                    (&mut _a_heap, &mut _b_heap, &mut _a_vv, &mut _b_vv, false)
                };

                while !a_heap.is_empty()
                    && a_heap.peek().map(|x| x.lamport).unwrap_or(0)
                        >= b_heap.peek().map(|x| x.lamport).unwrap_or(0)
                {
                    let a = a_heap.pop().unwrap();
                    let id = a.id;
                    if let Some(counter_end) = b_vv.get(&id.client_id) {
                        if id.counter < *counter_end {
                            return Some(id);
                        }
                    }

                    // if swapped {
                    //     println!("A");
                    // } else {
                    //     println!("B");
                    // }
                    // dbg!(&a);

                    #[cfg(debug_assertions)]
                    {
                        if let Some(v) = a_vv.get(&a.id.client_id) {
                            assert!(*v > a.id.counter)
                        }
                    }

                    for dep_id in a.deps {
                        let dep = self.get(*dep_id).unwrap();
                        a_heap.push(OrdId {
                            id: *dep_id,
                            lamport: dep.get_lamport_from_counter(dep_id.counter),
                            deps: dep.deps(),
                        });

                        if let Some(v) = a_vv.get_mut(&dep_id.client_id) {
                            if *v < dep_id.counter + 1 {
                                *v = dep_id.counter + 1;
                            }
                        } else {
                            a_vv.insert(dep_id.client_id, dep_id.counter + 1);
                        }
                    }
                }
            }

            None
        }
    }
}

fn update_frontier(frontier: &mut Vec<ID>, new_node_id: ID, new_node_deps: &[ID]) {
    frontier.retain(|x| {
        if x.client_id == new_node_id.client_id && x.counter <= new_node_id.counter {
            return false;
        }

        !new_node_deps
            .iter()
            .any(|y| y.client_id == x.client_id && y.counter >= x.counter)
    });

    // nodes from the same client with `counter < new_node_id.counter`
    // are filtered out from frontier.
    if frontier
        .iter()
        .all(|x| x.client_id != new_node_id.client_id)
    {
        frontier.push(new_node_id);
    }
}
