//! DAG (Directed Acyclic Graph) is a common data structure in distributed system.
//!
//! This mod contains the DAGs in our CRDT. It's not a general DAG, it has some specific properties that
//! we used to optimize the speed:
//! - Each node has lamport clock.
//! - Each node has its ID (client_id, counter).
//! - We use ID to refer to node rather than content addressing (hash)
//!
use std::{
    collections::{BinaryHeap, HashMap, HashSet, VecDeque},
    ops::Range,
};

use fxhash::{FxHashMap, FxHashSet};
mod iter;
mod mermaid;
#[cfg(test)]
mod test;

use crate::{
    change::Lamport,
    id::{ClientID, Counter, ID},
    span::{CounterSpan, IdSpan},
    version::VersionVector,
};

use self::{
    iter::{iter_dag, iter_dag_with_vv, DagIterator, DagIteratorVV},
    mermaid::dag_to_mermaid,
};

pub(crate) trait DagNode {
    fn id_start(&self) -> ID;
    fn lamport_start(&self) -> Lamport;
    fn len(&self) -> usize;
    fn deps(&self) -> &Vec<ID>;

    #[inline]
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    fn id_span(&self) -> IdSpan {
        let id = self.id_start();
        IdSpan {
            client_id: id.client_id,
            counter: CounterSpan::new(id.counter, id.counter + self.len() as Counter),
        }
    }

    /// inclusive end
    #[inline]
    fn id_end(&self) -> ID {
        let id = self.id_start();
        ID {
            client_id: id.client_id,
            counter: id.counter + self.len() as Counter - 1,
        }
    }

    #[inline]
    fn get_lamport_from_counter(&self, c: Counter) -> Lamport {
        self.lamport_start() + c as Lamport - self.id_start().counter as Lamport
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Path {
    retreat: Vec<IdSpan>,
    forward: Vec<IdSpan>,
}

#[allow(clippy::ptr_arg)]
fn reverse_path(path: &mut Vec<IdSpan>) {
    path.reverse();
    for span in path.iter_mut() {
        span.counter.reverse();
    }
}

/// Dag (Directed Acyclic Graph).
///
/// We have following invariance in DAG
/// - All deps' lamports are smaller than current node's lamport
pub(crate) trait Dag {
    type Node: DagNode;

    fn get(&self, id: ID) -> Option<&Self::Node>;

    #[inline]
    fn contains(&self, id: ID) -> bool {
        self.vv().includes(id)
    }

    fn frontier(&self) -> &[ID];
    fn roots(&self) -> Vec<&Self::Node>;
    fn vv(&self) -> VersionVector;

    //
    // TODO: Maybe use Result return type
    // TODO: Maybe we only need one heap?
    // TODO: benchmark
    // how to test better?
    // - converge through other nodes
    //
    /// only returns a single root.
    /// but the least common ancestor may be more than one root.
    /// But that is a rare case.
    ///
    #[inline]
    fn find_common_ancestor(&self, a_id: ID, b_id: ID) -> Option<ID> {
        find_common_ancestor(
            &|id| self.get(id).map(|x| x as &dyn DagNode),
            a_id,
            b_id,
            |_, _, _| {},
        )
    }

    /// TODO: we probably need cache to speedup this
    #[inline]
    fn get_vv(&self, id: ID) -> VersionVector {
        get_version_vector(&|id| self.get(id).map(|x| x as &dyn DagNode), id)
    }

    #[inline]
    fn find_path(&self, from: ID, to: ID) -> Option<Path> {
        let mut ans: Option<Path> = None;

        fn get_rev_path(target: ID, from: ID, to_from_map: &FxHashMap<ID, ID>) -> Vec<IdSpan> {
            let mut last_visited: Option<ID> = None;
            let mut a_rev_path = vec![];
            let mut node_id = target;
            node_id = *to_from_map.get(&node_id).unwrap();
            loop {
                if let Some(last_id) = last_visited {
                    if last_id.client_id == node_id.client_id {
                        debug_assert!(last_id.counter < node_id.counter);
                        a_rev_path.push(IdSpan {
                            client_id: last_id.client_id,
                            counter: CounterSpan::new(last_id.counter, node_id.counter + 1),
                        });
                        last_visited = None;
                    } else {
                        a_rev_path.push(IdSpan {
                            client_id: last_id.client_id,
                            counter: CounterSpan::new(last_id.counter, last_id.counter + 1),
                        });
                        last_visited = Some(node_id);
                    }
                } else {
                    last_visited = Some(node_id);
                }

                if node_id == from {
                    break;
                }

                node_id = *to_from_map.get(&node_id).unwrap();
            }

            if let Some(last_id) = last_visited {
                a_rev_path.push(IdSpan {
                    client_id: last_id.client_id,
                    counter: CounterSpan::new(last_id.counter, last_id.counter + 1),
                });
            }

            a_rev_path
        }

        find_common_ancestor(
            &|id| self.get(id).map(|x| x as &dyn DagNode),
            from,
            to,
            |ancestor, a_path, b_path| {
                let mut a_path = get_rev_path(ancestor, from, a_path);
                let b_path = get_rev_path(ancestor, to, b_path);
                reverse_path(&mut a_path);
                ans = Some(Path {
                    retreat: a_path,
                    forward: b_path,
                });
            },
        );

        ans
    }

    fn iter_with_vv(&self) -> DagIteratorVV<'_, Self::Node>
    where
        Self: Sized,
    {
        iter_dag_with_vv(self)
    }

    fn iter(&self) -> DagIterator<'_, Self::Node>
    where
        Self: Sized,
    {
        iter_dag(self)
    }

    /// You can visualize and generate img link at https://mermaid.live/
    #[inline]
    fn mermaid(&self) -> String
    where
        Self: Sized,
    {
        dag_to_mermaid(self)
    }
}

fn get_version_vector<'a, Get>(get: &'a Get, id: ID) -> VersionVector
where
    Get: Fn(ID) -> Option<&'a dyn DagNode>,
{
    let mut vv = VersionVector::new();
    let mut visited: FxHashSet<ID> = FxHashSet::default();
    vv.insert(id.client_id, id.counter + 1);
    let node = get(id).unwrap();

    if node.deps().is_empty() {
        return vv;
    }

    let mut stack = Vec::new();
    for dep in node.deps() {
        stack.push(dep);
    }

    while !stack.is_empty() {
        let node_id = *stack.pop().unwrap();
        let node = get(node_id).unwrap();
        let node_id_start = node.id_start();
        if !visited.contains(&node_id_start) {
            vv.try_update_end(node_id);
            for dep in node.deps() {
                if !visited.contains(dep) {
                    stack.push(dep);
                }
            }

            visited.insert(node_id_start);
        }
    }

    vv
}

// fn mermaid<T>(dag: &impl Dag<Node = T>) -> String {
//     dag
// }

fn find_common_ancestor<'a, F, G>(get: &'a F, a_id: ID, b_id: ID, mut on_found: G) -> Option<ID>
where
    F: Fn(ID) -> Option<&'a dyn DagNode>,
    G: FnMut(ID, &FxHashMap<ID, ID>, &FxHashMap<ID, ID>),
{
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
        // Invariant: every op id inserted to the a_heap is a key in a_path map, except for a_id
        let mut _a_heap: BinaryHeap<OrdId> = BinaryHeap::new();
        // Likewise
        let mut _b_heap: BinaryHeap<OrdId> = BinaryHeap::new();
        // FxHashMap<To, From> is used to track the deps path of each node
        let mut _a_path: FxHashMap<ID, ID> = FxHashMap::default();
        let mut _b_path: FxHashMap<ID, ID> = FxHashMap::default();
        {
            let a = get(a_id).unwrap();
            let b = get(b_id).unwrap();
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
            let (a_heap, b_heap, a_vv, b_vv, a_path, b_path, _swapped) = if _a_heap.is_empty()
                || (_a_heap.peek().map(|x| x.lamport).unwrap_or(0)
                    < _b_heap.peek().map(|x| x.lamport).unwrap_or(0))
            {
                // swap
                (
                    &mut _b_heap,
                    &mut _a_heap,
                    &mut _b_vv,
                    &mut _a_vv,
                    &mut _b_path,
                    &mut _a_path,
                    true,
                )
            } else {
                (
                    &mut _a_heap,
                    &mut _b_heap,
                    &mut _a_vv,
                    &mut _b_vv,
                    &mut _a_path,
                    &mut _b_path,
                    false,
                )
            };

            while !a_heap.is_empty()
                && a_heap.peek().map(|x| x.lamport).unwrap_or(0)
                    >= b_heap.peek().map(|x| x.lamport).unwrap_or(0)
            {
                let a = a_heap.pop().unwrap();
                let id = a.id;
                if let Some(counter_end) = b_vv.get(&id.client_id) {
                    if id.counter < *counter_end {
                        b_path
                            .entry(id)
                            .or_insert_with(|| ID::new(id.client_id, counter_end - 1));

                        on_found(id, &_a_path, &_b_path);
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
                    if a_path.contains_key(dep_id) {
                        continue;
                    }

                    let dep = get(*dep_id).unwrap();
                    a_heap.push(OrdId {
                        id: *dep_id,
                        lamport: dep.get_lamport_from_counter(dep_id.counter),
                        deps: dep.deps(),
                    });
                    a_path.insert(*dep_id, a.id);
                    if dep.id_start() != *dep_id {
                        a_path.insert(dep.id_start(), *dep_id);
                    }

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
