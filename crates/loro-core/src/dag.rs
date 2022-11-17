//! DAG (Directed Acyclic Graph) is a common data structure in distributed system.
//!
//! This mod contains the DAGs in our CRDT. It's not a general DAG, it has some specific properties that
//! we used to optimize the speed:
//! - Each node has lamport clock.
//! - Each node has its ID (client_id, counter).
//! - We use ID to refer to node rather than content addressing (hash)
//!
use std::{
    borrow::Cow,
    collections::{BinaryHeap, HashMap},
    fmt::Debug,
};

#[allow(unused)]
use colored::Colorize;
use fxhash::{FxHashMap, FxHashSet};
use rle::{HasLength, Sliceable};
use smallvec::{smallvec, SmallVec};
mod iter;
mod mermaid;
#[cfg(test)]
mod test;

use crate::{
    change::Lamport,
    debug_log,
    id::{ClientID, Counter, ID},
    span::{CounterSpan, HasId, HasIdSpan, HasLamport, HasLamportSpan, IdSpan},
    version::{IdSpanVector, VersionVector, VersionVectorDiff},
};

use self::{
    iter::{iter_dag, iter_dag_with_vv, DagCausalIter, DagIterator, DagIteratorVV},
    mermaid::dag_to_mermaid,
};

pub(crate) trait DagNode: HasLamport + HasId + HasLength + Debug + Sliceable {
    fn deps(&self) -> &[ID];

    #[inline]
    fn get_lamport_from_counter(&self, c: Counter) -> Lamport {
        self.lamport() + c as Lamport - self.id_start().counter as Lamport
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
    /// Slow, should probably only use on dev
    fn get_vv(&self, id: ID) -> VersionVector;
    fn find_path(&self, from: &[ID], to: &[ID]) -> VersionVectorDiff;
    fn contains(&self, id: ID) -> bool;
    fn iter_causal(&self, from: &[ID], target: IdSpanVector) -> DagCausalIter<'_, Self>
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
    #[inline]
    fn find_common_ancestor(&self, a_id: &[ID], b_id: &[ID]) -> SmallVec<[ID; 2]> {
        // TODO: perf: make it also return the spans to reach common_ancestors
        find_common_ancestor(&|id| self.get(id), a_id, b_id)
    }

    #[inline]
    fn contains(&self, id: ID) -> bool {
        self.vv().includes_id(id)
    }

    #[inline]
    fn get_vv(&self, id: ID) -> VersionVector {
        get_version_vector(&|id| self.get(id), id)
    }

    fn find_path(&self, from: &[ID], to: &[ID]) -> VersionVectorDiff {
        let mut ans = VersionVectorDiff::default();
        debug_log!(
            "{}",
            format!("FINDPATH from={:?} to={:?}", from, to).green()
        );
        if from == to {
            return ans;
        }
        if from.len() == 1 && to.len() == 1 {
            let from = from[0];
            let to = to[0];
            if from.client_id == to.client_id {
                let from_span = self.get(from).unwrap();
                let to_span = self.get(to).unwrap();
                if std::ptr::eq(from_span, to_span) {
                    if from.counter < to.counter {
                        ans.right.insert(
                            from.client_id,
                            CounterSpan::new(from.counter + 1, to.counter + 1),
                        );
                    } else {
                        ans.left.insert(
                            from.client_id,
                            CounterSpan::new(to.counter + 1, from.counter + 1),
                        );
                    }
                    return ans;
                }

                if from_span.deps().len() == 1 && to_span.contains_id(from_span.deps()[0]) {
                    ans.left.insert(
                        from.client_id,
                        CounterSpan::new(to.counter + 1, from.counter + 1),
                    );
                    return ans;
                }

                if to_span.deps().len() == 1 && from_span.contains_id(to_span.deps()[0]) {
                    ans.right.insert(
                        from.client_id,
                        CounterSpan::new(from.counter + 1, to.counter + 1),
                    );
                    return ans;
                }
            }
        }

        _find_common_ancestor(
            &|v| self.get(v),
            from,
            to,
            &mut |span, node_type| match node_type {
                NodeType::A => ans.merge_left(span),
                NodeType::B => ans.merge_right(span),
                NodeType::Shared => {
                    ans.subtract_start_left(span);
                    ans.subtract_start_right(span);
                }
            },
            true,
        );

        // dbg!(from, to, &ans);
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
    fn iter_causal(&self, from: &[ID], target: IdSpanVector) -> DagCausalIter<'_, Self>
    where
        Self: Sized,
    {
        DagCausalIter::new(self, from.into(), target)
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

    let mut stack = Vec::with_capacity(node.deps().len());
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
    deps: Cow<'a, [ID]>,
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
            deps: Cow::Borrowed(span.deps()),
            len: (id.counter - span_id.counter) as usize + 1,
        })
    }

    #[inline]
    fn get_min(&self) -> OrdIdSpan<'a> {
        OrdIdSpan {
            id: self.id,
            lamport: self.lamport,
            deps: Cow::Borrowed(&[]),
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
    if a_id.is_empty() || b_id.is_empty() {
        return smallvec::smallvec![];
    }

    _find_common_ancestor_new(get, a_id, b_id)
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

        for &dep_id in node.deps.as_ref() {
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
                        deps: Cow::Borrowed(&[]),
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

fn _find_common_ancestor_new<'a, F, D>(get: &'a F, left: &[ID], right: &[ID]) -> SmallVec<[ID; 2]>
where
    D: DagNode + 'a,
    F: Fn(ID) -> Option<&'a D>,
{
    if left.len() == 1 && right.len() == 1 {
        let left = left[0];
        let right = right[0];
        if left.client_id == right.client_id {
            let left_span = get(left).unwrap();
            let right_span = get(right).unwrap();
            if std::ptr::eq(left_span, right_span) {
                if left.counter < right.counter {
                    return smallvec![left];
                } else {
                    return smallvec![right];
                }
            }

            if left_span.deps().len() == 1 && right_span.contains_id(left_span.deps()[0]) {
                return smallvec![right];
            }

            if right_span.deps().len() == 1 && left_span.contains_id(right_span.deps()[0]) {
                return smallvec![left];
            }
        }
    }

    let mut ans: SmallVec<[ID; 2]> = Default::default();
    let mut queue: BinaryHeap<(SmallVec<[OrdIdSpan; 1]>, NodeType)> = BinaryHeap::new();

    fn ids_to_ord_id_spans<'a, D: DagNode + 'a, F: Fn(ID) -> Option<&'a D>>(
        ids: &[ID],
        get: &'a F,
    ) -> SmallVec<[OrdIdSpan<'a>; 1]> {
        let mut ans: SmallVec<[OrdIdSpan<'a>; 1]> = ids
            .iter()
            .map(|&id| OrdIdSpan::from_dag_node(id, get).unwrap())
            .collect();
        if ans.len() > 1 {
            ans.sort();
            ans.reverse();
        }

        ans
    }

    queue.push((ids_to_ord_id_spans(left, get), NodeType::A));
    queue.push((ids_to_ord_id_spans(right, get), NodeType::B));
    while let Some((mut node, mut node_type)) = queue.pop() {
        while let Some((other_node, other_type)) = queue.peek() {
            if node == *other_node {
                if node_type != *other_type {
                    node_type = NodeType::Shared;
                }

                queue.pop();
            } else {
                break;
            }
        }

        if queue.is_empty() {
            if node_type == NodeType::Shared {
                ans = node.into_iter().map(|x| x.id_last()).collect();
            }

            break;
        }

        if node.len() > 1 {
            for node in node.drain(1..node.len()) {
                queue.push((smallvec::smallvec![node], node_type));
            }
        }

        if let Some(other) = queue.peek() {
            if other.0.len() == 1
                && node[0].contains_id(other.0[0].id_last())
                && node_type != other.1
            {
                node[0].len = (other.0[0].id_last().counter - node[0].id.counter + 1) as usize;
                queue.push((node, node_type));
                continue;
            }

            if node[0].len > 1 {
                if other.0[0].lamport_last() > node[0].lamport {
                    node[0].len = (other.0[0].lamport_last() - node[0].lamport)
                        .min(node[0].len as u32 - 1) as usize;
                    queue.push((node, node_type));
                    continue;
                } else {
                    node[0].len = 1;
                    queue.push((node, node_type));
                    continue;
                }
            }
        }

        if node[0].deps.len() > 0 {
            queue.push((ids_to_ord_id_spans(node[0].deps.as_ref(), get), node_type));
        } else {
            break;
        }
    }

    ans
}

pub fn remove_included_frontiers(frontiers: &mut VersionVector, new_change_deps: &[ID]) {
    for dep in new_change_deps.iter() {
        if let Some(last) = frontiers.get_last(dep.client_id) {
            if last <= dep.counter {
                frontiers.remove(&dep.client_id);
            }
        }
    }
}
