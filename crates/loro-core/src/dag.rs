//! DAG (Directed Acyclic Graph) is a common data structure in distributed system.
//!
//! This mod contains the DAGs in our CRDT. It's not a general DAG, it has some specific properties that
//! we used to optimize the speed:
//! - Each node has lamport clock.
//! - Each node has its ID (client_id, counter).
//! - We use ID to refer to node rather than content addressing (hash)
//!
use std::{
    collections::{BinaryHeap, HashMap},
    fmt::Debug,
    time::Instant,
};

use bit_vec::BitVec;
use fxhash::{FxHashMap, FxHashSet};
use rle::{HasLength, Sliceable};
use smallvec::SmallVec;
mod iter;
mod mermaid;
#[cfg(test)]
mod test;

use crate::{
    change::Lamport,
    id::{ClientID, Counter, ID},
    span::{HasId, HasIdSpan, HasLamport, HasLamportSpan, IdSpan},
    version::{IdSpanVector, VersionVector, VersionVectorDiff},
};

use self::{
    iter::{iter_dag, iter_dag_with_vv, DagIterator, DagIteratorVV, DagPartialIter},
    mermaid::dag_to_mermaid,
};

// TODO: use HasId, HasLength
pub(crate) trait DagNode: HasLamport + HasId + HasLength + Debug + Sliceable {
    fn deps(&self) -> &[ID];

    #[inline]
    fn get_lamport_from_counter(&self, c: Counter) -> Lamport {
        self.lamport() + c as Lamport - self.id_start().counter as Lamport
    }
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

    fn frontier(&self) -> &[ID];
    fn vv(&self) -> VersionVector;
}

pub(crate) trait DagUtils: Dag {
    fn find_common_ancestor(&self, a_id: &[ID], b_id: &[ID]) -> SmallVec<[ID; 2]>;
    fn get_vv(&self, id: ID) -> VersionVector;
    fn find_path(&self, from: &[ID], to: &[ID]) -> VersionVectorDiff;
    fn contains(&self, id: ID) -> bool;
    fn iter_partial(&self, from: &[ID], target: IdSpanVector) -> DagPartialIter<'_, Self>
    where
        Self: Sized;
    fn iter(&self) -> DagIterator<'_, Self::Node>
    where
        Self: Sized;
    fn iter_with_vv(&self) -> DagIteratorVV<'_, Self::Node>
    where
        Self: Sized;
    fn mermaid(&self) -> String
    where
        Self: Sized;
}

impl<T: Dag + ?Sized> DagUtils for T {
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
    fn find_common_ancestor(&self, a_id: &[ID], b_id: &[ID]) -> SmallVec<[ID; 2]> {
        find_common_ancestor(&|id| self.get(id), a_id, b_id)
    }

    #[inline]
    fn contains(&self, id: ID) -> bool {
        self.vv().includes_id(id)
    }

    /// TODO: we probably need cache to speedup this
    #[inline]
    fn get_vv(&self, id: ID) -> VersionVector {
        get_version_vector(&|id| self.get(id), id)
    }

    fn find_path(&self, from: &[ID], to: &[ID]) -> VersionVectorDiff {
        let mut ans = VersionVectorDiff::default();
        _find_common_ancestor(
            &|v| self.get(v),
            from,
            to,
            &mut |span, node_type| {
                // dbg!(span, node_type);
                match node_type {
                    NodeType::A => ans.merge_left(span),
                    NodeType::B => ans.merge_right(span),
                    NodeType::Shared => {
                        ans.subtract_start_left(span);
                        ans.subtract_start_right(span);
                    }
                }
            },
            true,
        );

        ans
    }

    #[inline(always)]
    fn iter_with_vv(&self) -> DagIteratorVV<'_, Self::Node>
    where
        Self: Sized,
    {
        iter_dag_with_vv(self)
    }

    #[inline(always)]
    fn iter_partial(&self, from: &[ID], target: IdSpanVector) -> DagPartialIter<'_, Self>
    where
        Self: Sized,
    {
        DagPartialIter::new(self, from.into(), target)
    }

    #[inline(always)]
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

fn get_version_vector<'a, Get, D>(get: &'a Get, id: ID) -> VersionVector
where
    Get: Fn(ID) -> Option<&'a D>,
    D: DagNode + 'a,
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
            vv.try_update_last(node_id);
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

#[derive(Debug, PartialEq, Eq)]
struct OrdIdSpan<'a> {
    id: ID,
    lamport: Lamport,
    len: usize,
    deps: &'a [ID],
}

impl<'a> HasLength for OrdIdSpan<'a> {
    fn content_len(&self) -> usize {
        self.len
    }
}

impl<'a> HasId for OrdIdSpan<'a> {
    fn id_start(&self) -> ID {
        self.id
    }
}

impl<'a> HasLamport for OrdIdSpan<'a> {
    fn lamport(&self) -> Lamport {
        self.lamport
    }
}

impl<'a> PartialOrd for OrdIdSpan<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(
            self.lamport_last()
                .cmp(&other.lamport_last())
                .then(self.id_last().cmp(&other.id_last())),
        )
    }
}

impl<'a> Ord for OrdIdSpan<'a> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.lamport_last()
            .cmp(&other.lamport_last())
            .then(self.id_last().cmp(&other.id_last()))
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum NodeType {
    A,
    B,
    Shared,
}

impl<'a> OrdIdSpan<'a> {
    #[inline]
    fn from_dag_node<D, F>(id: ID, get: &'a F) -> Option<OrdIdSpan>
    where
        D: DagNode + 'a,
        F: Fn(ID) -> Option<&'a D>,
    {
        let span = get(id)?;
        let span_id = span.id_start();
        Some(OrdIdSpan {
            id: span_id,
            lamport: span.lamport(),
            deps: span.deps(),
            len: (id.counter - span_id.counter) as usize + 1,
        })
    }

    #[inline]
    fn get_min(&self) -> OrdIdSpan<'a> {
        OrdIdSpan {
            id: self.id,
            lamport: self.lamport,
            deps: &[],
            len: 1,
        }
    }
}

#[inline(always)]
fn find_common_ancestor<'a, F, D>(get: &'a F, a_id: &[ID], b_id: &[ID]) -> SmallVec<[ID; 2]>
where
    D: DagNode + 'a,
    F: Fn(ID) -> Option<&'a D>,
{
    let mut ids = Vec::with_capacity(a_id.len() + b_id.len());
    for id in a_id {
        ids.push(*id);
    }
    for id in b_id {
        ids.push(*id);
    }

    _find_common_ancestor_new(get, &ids)
        .into_iter()
        .map(|x| ID::new(x.0, x.1))
        .collect()
}

/// - deep whether keep searching until the min of non-shared node is found
fn _find_common_ancestor<'a, F, D, G>(
    get: &'a F,
    a_ids: &[ID],
    b_ids: &[ID],
    notify: &mut G,
    find_path: bool,
) -> FxHashMap<ClientID, Counter>
where
    D: DagNode + 'a,
    F: Fn(ID) -> Option<&'a D>,
    G: FnMut(IdSpan, NodeType),
{
    let mut ans: FxHashMap<ClientID, Counter> = Default::default();
    let mut queue: BinaryHeap<(OrdIdSpan, NodeType)> = BinaryHeap::new();
    for id in a_ids {
        queue.push((OrdIdSpan::from_dag_node(*id, get).unwrap(), NodeType::A));
    }
    for id in b_ids {
        queue.push((OrdIdSpan::from_dag_node(*id, get).unwrap(), NodeType::B));
    }
    let mut visited: HashMap<ClientID, (Counter, NodeType), _> = FxHashMap::default();
    // invariants in this method:
    //
    // - visited's (client, counters) are subset of max(version_vector_a, version_vector_b)
    // - visited's node type reflecting whether we found the shared node of this client
    // - ans's client id never repeat
    // - nodes with the same id will only be visited once
    // - we may visit nodes that are before the common ancestors

    // type count in the queue. if both are zero, we can stop
    let mut a_count = a_ids.len();
    let mut b_count = b_ids.len();
    let mut min = None;
    while let Some((node, mut node_type)) = queue.pop() {
        match node_type {
            NodeType::A => a_count -= 1,
            NodeType::B => b_count -= 1,
            NodeType::Shared => {}
        }

        if node_type != NodeType::Shared {
            if let Some(min) = &mut min {
                let node_start = node.get_min();
                if node_start < *min {
                    *min = node_start;
                }
            } else {
                min = Some(node.get_min())
            }
        }

        // pop the same node in the queue
        while let Some((other_node, other_type)) = queue.peek() {
            if node.id_span() == other_node.id_span() {
                if node_type == *other_type {
                    match node_type {
                        NodeType::A => a_count -= 1,
                        NodeType::B => b_count -= 1,
                        NodeType::Shared => {}
                    }
                } else {
                    if node_type != NodeType::Shared {
                        if visited.get(&node.id.client_id).map(|(_, t)| *t)
                            != Some(NodeType::Shared)
                        {
                            ans.insert(node.id.client_id, other_node.id_last().counter);
                        }
                        node_type = NodeType::Shared;
                    }
                    match other_type {
                        NodeType::A => a_count -= 1,
                        NodeType::B => b_count -= 1,
                        NodeType::Shared => {}
                    }
                }

                queue.pop();
            } else {
                break;
            }
        }

        // detect whether client is visited by other
        if let Some((ctr, visited_type)) = visited.get_mut(&node.id.client_id) {
            debug_assert!(*ctr >= node.id_last().counter);
            if *visited_type == NodeType::Shared {
                node_type = NodeType::Shared;
            } else if *visited_type != node_type {
                // if node_type is shared, ans should already contains it or its descendance
                if node_type != NodeType::Shared {
                    ans.insert(node.id.client_id, node.id_last().counter);
                }
                *visited_type = NodeType::Shared;
                node_type = NodeType::Shared;
            }
        } else {
            visited.insert(node.id.client_id, (node.id_last().counter, node_type));
        }

        // if this is not shared, the end of the span must be only reachable from A, or only reachable from B.
        // but the begin of the span may be reachable from both A and B
        notify(node.id_span(), node_type);

        match node_type {
            NodeType::A => a_count += node.deps.len(),
            NodeType::B => b_count += node.deps.len(),
            NodeType::Shared => {}
        }

        if a_count == 0
            && b_count == 0
            && (!find_path || min.is_none() || &node <= min.as_ref().unwrap())
        {
            if node_type != NodeType::Shared {
                ans.clear();
            }

            break;
        }

        for &dep_id in node.deps {
            queue.push((OrdIdSpan::from_dag_node(dep_id, get).unwrap(), node_type));
        }

        if node_type != NodeType::Shared {
            if queue.is_empty() {
                ans.clear();
                break;
            }
            if node.deps.is_empty() && !find_path {
                if node.len == 1 {
                    ans.clear();
                    break;
                }

                match node_type {
                    NodeType::A => a_count += 1,
                    NodeType::B => b_count += 1,
                    NodeType::Shared => {}
                }

                queue.push((
                    OrdIdSpan {
                        deps: &[],
                        id: node.id,
                        len: 1,
                        lamport: node.lamport,
                    },
                    node_type,
                ));
            }
        }
    }

    ans
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

// TODO: BitVec may be too slow here
fn _find_common_ancestor_new<'a, F, D>(get: &'a F, ids: &[ID]) -> FxHashMap<ClientID, Counter>
where
    D: DagNode + 'a,
    F: Fn(ID) -> Option<&'a D>,
{
    let mut ans = FxHashMap::default();
    if ids.len() <= 1 {
        for id in ids {
            ans.insert(id.client_id, id.counter);
        }

        return ans;
    }

    let mut queue: BinaryHeap<(OrdIdSpan, BitVec)> = BinaryHeap::new();
    let mut shared_num = 0;
    let mut min = None;
    let mut visited: HashMap<ClientID, (Counter, BitVec), _> = FxHashMap::default();
    for (i, id) in ids.iter().enumerate() {
        let mut bitmap = BitVec::from_elem(ids.len(), false);
        bitmap.set(i, true);
        queue.push((OrdIdSpan::from_dag_node(*id, get).unwrap(), bitmap));
    }

    while let Some((this_node, mut this_map)) = queue.pop() {
        let is_shared_from_start = this_map.all();
        let mut is_shared = is_shared_from_start;

        if is_shared_from_start {
            shared_num -= 1;
        }

        // pop the same node in the queue
        while let Some((other_node, other_map)) = queue.peek() {
            if this_node.id_span() == other_node.id_span() {
                if other_map.all() {
                    shared_num -= 1;
                }

                if !is_shared && this_map.or(other_map) && this_map.all() {
                    is_shared = true;
                    ans.insert(this_node.id.client_id, other_node.ctr_last());
                }

                queue.pop();
            } else {
                break;
            }
        }

        // detect whether client is visited by other
        if let Some((ctr, visited_map)) = visited.get_mut(&this_node.id.client_id) {
            debug_assert!(*ctr >= this_node.id_last().counter);
            if visited_map.all() {
                is_shared = true;
            } else if !is_shared && visited_map.or(&this_map) && visited_map.all() {
                ans.insert(this_node.id.client_id, this_node.id_last().counter);
                is_shared = true;
            }
        } else {
            visited.insert(
                this_node.id.client_id,
                (this_node.id_last().counter, this_map.clone()),
            );
        }

        if shared_num == queue.len() && (min.is_none() || &this_node <= min.as_ref().unwrap()) {
            if !is_shared {
                ans.clear();
            }

            break;
        }

        for &dep_id in this_node.deps {
            let node = OrdIdSpan::from_dag_node(dep_id, get).unwrap();
            if let Some(min) = &mut min {
                let node_start = node.get_min();
                if node_start < *min {
                    *min = node_start;
                }
            } else {
                min = Some(node.get_min())
            }

            queue.push((node, this_map.clone()));
        }

        if is_shared {
            shared_num += this_node.deps.len()
        }

        if !is_shared {
            if queue.is_empty() {
                ans.clear();
                break;
            }

            if this_node.deps.is_empty() {
                if this_node.len == 1 {
                    ans.clear();
                    break;
                }

                queue.push((
                    OrdIdSpan {
                        deps: &[],
                        id: this_node.id,
                        len: this_node.len - 1,
                        lamport: this_node.lamport,
                    },
                    this_map,
                ));
            }
        }
    }

    ans
}
