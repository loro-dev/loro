use std::{
    collections::{BinaryHeap, HashMap, VecDeque},
    ops::Range,
};

use fxhash::FxHashMap;
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

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn dag_id_span(&self) -> IdSpan {
        let id = self.dag_id_start();
        IdSpan {
            client_id: id.client_id,
            counter: CounterSpan::new(id.counter, id.counter + self.len() as Counter),
        }
    }
}

pub(crate) trait Dag {
    type Node: DagNode;

    fn get(&self, id: ID) -> Option<&Self::Node>;
    fn contains(&self, id: ID) -> bool;
    fn frontier(&self) -> &[ID];
    fn roots(&self) -> Vec<&Self::Node>;

    fn get_common_ancestor(&self, a: ID, b: ID) -> Option<ID> {
        if a.client_id == b.client_id {
            if a.counter <= b.counter {
                Some(a)
            } else {
                Some(b)
            }
        } else {
            #[derive(Debug, PartialEq, Eq)]
            struct OrdId {
                id: ID,
                lamport: Lamport,
            }

            impl PartialOrd for OrdId {
                fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                    Some(self.lamport.cmp(&other.lamport))
                }
            }

            impl Ord for OrdId {
                fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                    self.lamport.cmp(&other.lamport)
                }
            }

            let mut a_map: HashMap<ClientID, Range<Counter>, _> = FxHashMap::default();
            let mut b_map: HashMap<ClientID, Range<Counter>, _> = FxHashMap::default();
            let mut a_heap: BinaryHeap<OrdId> = BinaryHeap::new();
            let mut b_heap: BinaryHeap<OrdId> = BinaryHeap::new();
            {
                let a = self.get(a).unwrap();
                let b = self.get(b).unwrap();
                a_heap.push(OrdId {
                    id: a.dag_id_start(),
                    lamport: a.lamport_start() + a.len() as Lamport,
                });
                b_heap.push(OrdId {
                    id: b.dag_id_start(),
                    lamport: b.lamport_start() + b.len() as Lamport,
                });
            }

            while !a_heap.is_empty() || !b_heap.is_empty() {
                let (a_heap, b_heap, a_map, b_map) =
                    if a_heap.peek().map(|x| x.lamport).unwrap_or(0)
                        < b_heap.peek().map(|x| x.lamport).unwrap_or(0)
                    {
                        // swap
                        (&mut b_heap, &mut a_heap, &mut b_map, &mut a_map)
                    } else {
                        (&mut a_heap, &mut b_heap, &mut a_map, &mut b_map)
                    };

                while !a_heap.is_empty()
                    && a_heap.peek().map(|x| x.lamport).unwrap_or(0)
                        >= b_heap.peek().map(|x| x.lamport).unwrap_or(0)
                {
                    let id = a_heap.pop().unwrap().id;
                    if let Some(range) = b_map.get(&id.client_id) {
                        if range.contains(&id.counter) {
                            return Some(id);
                        }
                    }

                    let a = self.get(id).unwrap();
                    for dep in a.deps() {
                        a_heap.push(OrdId {
                            id: *dep,
                            lamport: a.lamport_start() + a.len() as Lamport,
                        });
                    }
                    if let Some(range) = a_map.get_mut(&id.client_id) {
                        range.start = a.dag_id_start().counter;
                    } else {
                        let span = a.dag_id_span();
                        a_map.insert(id.client_id, span.counter.from..span.counter.to);
                    }
                }
            }

            None
        }
    }
}

fn update_frontier(frontier: &mut Vec<ID>, new_node_id: ID, new_node_deps: &[ID]) {
    frontier.retain(|x| {
        !new_node_deps
            .iter()
            .any(|y| y.client_id == x.client_id && y.counter >= x.counter)
    });
    frontier.push(new_node_id);
}
