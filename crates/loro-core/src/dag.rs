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
};

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
    fn len(&self) -> usize {
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
            deps: &self.deps[0..0],
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
    _find_common_ancestor(get, a_id, b_id, &mut |_, _| {}, false)
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
    deep: bool,
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
            && (!deep || min.is_none() || &node <= min.as_ref().unwrap())
        {
            break;
        }

        for &dep_id in node.deps {
            queue.push((OrdIdSpan::from_dag_node(dep_id, get).unwrap(), node_type));
        }

        if node_type != NodeType::Shared && queue.is_empty() {
            ans.clear();
        }
    }

    ans
}

fn find_common_ancestor_old<'a, F, G, D>(
    get: &'a F,
    a_id: ID,
    b_id: ID,
    mut on_found: G,
) -> Option<ID>
where
    D: DagNode + 'a,
    F: Fn(ID) -> Option<&'a D>,
    G: FnMut(ID, &FxHashMap<ID, ID>, &FxHashMap<ID, ID>),
{
    if a_id.client_id == b_id.client_id {
        if a_id.counter <= b_id.counter {
            Some(a_id)
        } else {
            Some(b_id)
        }
    } else {
        let mut _a_vv: HashMap<ClientID, Counter, _> = FxHashMap::default();
        let mut _b_vv: HashMap<ClientID, Counter, _> = FxHashMap::default();
        // Invariant: every op id inserted to the a_heap is a key in a_path map, except for a_id
        let mut _a_heap: BinaryHeap<OrdIdSpan> = BinaryHeap::new();
        // Likewise
        let mut _b_heap: BinaryHeap<OrdIdSpan> = BinaryHeap::new();
        // FxHashMap<To, From> is used to track the deps path of each node
        let mut _a_path: FxHashMap<ID, ID> = FxHashMap::default();
        let mut _b_path: FxHashMap<ID, ID> = FxHashMap::default();
        {
            let a = get(a_id).unwrap();
            let b = get(b_id).unwrap();
            _a_heap.push(OrdIdSpan {
                id: a_id,
                lamport: a.get_lamport_from_counter(a_id.counter),
                deps: a.deps(),
                len: 1,
            });
            _b_heap.push(OrdIdSpan {
                id: b_id,
                lamport: b.get_lamport_from_counter(b_id.counter),
                deps: b.deps(),
                len: 1,
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

                #[cfg(debug_assertions)]
                {
                    if let Some(v) = a_vv.get(&a.id.client_id) {
                        assert!(*v > a.id.counter)
                    }
                }

                for &dep_id in a.deps {
                    if a_path.contains_key(&dep_id) {
                        continue;
                    }

                    let dep = get(dep_id).unwrap();
                    a_heap.push(OrdIdSpan {
                        id: dep_id,
                        lamport: dep.get_lamport_from_counter(dep_id.counter),
                        deps: dep.deps(),
                        len: 1,
                    });
                    a_path.insert(dep_id, a.id);
                    if dep.id_start() != dep_id {
                        a_path.insert(dep.id_start(), dep_id);
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
