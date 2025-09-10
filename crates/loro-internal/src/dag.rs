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

use rustc_hash::{FxHashMap, FxHashSet};
use loro_common::IdSpanVector;
use rle::{HasLength, Sliceable};
use smallvec::SmallVec;
mod iter;
mod mermaid;
#[cfg(feature = "test_utils")]
mod test;
#[cfg(feature = "test_utils")]
pub use test::{fuzz_alloc_tree, Interaction};

use crate::{
    change::Lamport,
    diff_calc::DiffMode,
    id::{Counter, PeerID, ID},
    span::{CounterSpan, HasId, HasIdSpan, HasLamport, HasLamportSpan, IdSpan},
    version::{Frontiers, VersionVector, VersionVectorDiff},
};

use self::{
    iter::{iter_dag, iter_dag_with_vv, DagCausalIter, DagIterator, DagIteratorVV},
    mermaid::dag_to_mermaid,
};

pub(crate) trait DagNode: HasLamport + HasId + HasLength + Debug + Sliceable {
    fn deps(&self) -> &Frontiers;

    #[allow(unused)]
    #[inline]
    fn get_lamport_from_counter(&self, c: Counter) -> Lamport {
        self.lamport() + c as Lamport - self.id_start().counter as Lamport
    }
}

/// Dag (Directed Acyclic Graph).
///
/// We have following invariance in DAG
/// - All deps' lamports are smaller than current node's lamport
pub(crate) trait Dag: Debug {
    type Node: DagNode;

    fn get(&self, id: ID) -> Option<Self::Node>;
    #[allow(unused)]
    fn frontier(&self) -> &Frontiers;
    fn vv(&self) -> &VersionVector;
    fn contains(&self, id: ID) -> bool;
}

pub(crate) trait DagUtils: Dag {
    fn find_common_ancestor(&self, a_id: &Frontiers, b_id: &Frontiers) -> (Frontiers, DiffMode);
    /// Slow, should probably only use on dev
    #[allow(unused)]
    fn get_vv(&self, id: ID) -> VersionVector;
    #[allow(unused)]
    fn find_path(&self, from: &Frontiers, to: &Frontiers) -> VersionVectorDiff;
    fn iter_causal(&self, from: Frontiers, target: IdSpanVector) -> DagCausalIter<'_, Self>
    where
        Self: Sized;
    #[allow(unused)]
    fn iter(&self) -> DagIterator<'_, Self::Node>
    where
        Self: Sized;
    #[allow(unused)]
    fn iter_with_vv(&self) -> DagIteratorVV<'_, Self::Node>
    where
        Self: Sized;
    #[allow(unused)]
    fn mermaid(&self) -> String
    where
        Self: Sized;
}

impl<T: Dag + ?Sized> DagUtils for T {
    #[inline]
    fn find_common_ancestor(&self, a_id: &Frontiers, b_id: &Frontiers) -> (Frontiers, DiffMode) {
        // TODO: perf: make it also return the spans to reach common_ancestors
        find_common_ancestor(&|id| self.get(id), a_id, b_id)
    }

    #[inline]
    fn get_vv(&self, id: ID) -> VersionVector {
        get_version_vector(&|id| self.get(id), id)
    }

    fn find_path(&self, from: &Frontiers, to: &Frontiers) -> VersionVectorDiff {
        let mut ans = VersionVectorDiff::default();
        if from == to {
            return ans;
        }

        if from.len() == 1 && to.len() == 1 {
            let from = from.as_single().unwrap();
            let to = to.as_single().unwrap();
            if from.peer == to.peer {
                let from_span = self.get(from).unwrap();
                let to_span = self.get(to).unwrap();
                if from_span.id_start() == to_span.id_start() {
                    if from.counter < to.counter {
                        ans.forward.insert(
                            from.peer,
                            CounterSpan::new(from.counter + 1, to.counter + 1),
                        );
                    } else {
                        ans.retreat.insert(
                            from.peer,
                            CounterSpan::new(to.counter + 1, from.counter + 1),
                        );
                    }
                    return ans;
                }

                if from_span.deps().len() == 1
                    && to_span.contains_id(from_span.deps().as_single().unwrap())
                {
                    ans.retreat.insert(
                        from.peer,
                        CounterSpan::new(to.counter + 1, from.counter + 1),
                    );
                    return ans;
                }

                if to_span.deps().len() == 1
                    && from_span.contains_id(to_span.deps().as_single().unwrap())
                {
                    ans.forward.insert(
                        from.peer,
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
    fn iter_causal(&self, from: Frontiers, target: IdSpanVector) -> DagCausalIter<'_, Self>
    where
        Self: Sized,
    {
        DagCausalIter::new(self, from, target)
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

#[allow(dead_code)]
fn get_version_vector<'a, Get, D>(get: &'a Get, id: ID) -> VersionVector
where
    Get: Fn(ID) -> Option<D>,
    D: DagNode + 'a,
{
    let mut vv = VersionVector::new();
    let mut visited: FxHashSet<ID> = FxHashSet::default();
    vv.insert(id.peer, id.counter + 1);
    let node = get(id).unwrap();

    if node.deps().is_empty() {
        return vv;
    }

    let mut stack = Vec::with_capacity(node.deps().len());
    for dep in node.deps().iter() {
        stack.push(dep);
    }

    while let Some(node_id) = stack.pop() {
        let node = get(node_id).unwrap();
        let node_id_start = node.id_start();
        if !visited.contains(&node_id_start) {
            vv.try_update_last(node_id);
            for dep in node.deps().iter() {
                if !visited.contains(&dep) {
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
    deps: Cow<'a, Frontiers>,
}

impl HasLength for OrdIdSpan<'_> {
    fn content_len(&self) -> usize {
        self.len
    }
}

impl HasId for OrdIdSpan<'_> {
    fn id_start(&self) -> ID {
        self.id
    }
}

impl HasLamport for OrdIdSpan<'_> {
    fn lamport(&self) -> Lamport {
        self.lamport
    }
}

impl PartialOrd for OrdIdSpan<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrdIdSpan<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.lamport_last()
            .cmp(&other.lamport_last())
            .then(self.id.peer.cmp(&other.id.peer))
            // If they have the same last id, we want the shorter one to be greater;
            // Otherwise, find_common_ancestor won't work correctly. Because we may
            // lazily load the dag node, so sometimes the longer one should be broken
            // into smaller pieces but it's already pushed to the queue.
            .then(other.len.cmp(&self.len))
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
    fn from_dag_node<D, F>(id: ID, get: &'a F) -> Option<OrdIdSpan<'a>>
    where
        D: DagNode + 'a,
        F: Fn(ID) -> Option<D>,
    {
        let span = get(id)?;
        let span_id = span.id_start();
        Some(OrdIdSpan {
            id: span_id,
            lamport: span.lamport(),
            deps: Cow::Owned(span.deps().clone()),
            len: (id.counter - span_id.counter) as usize + 1,
        })
    }

    #[inline]
    fn get_min(&self) -> OrdIdSpan<'a> {
        OrdIdSpan {
            id: self.id,
            lamport: self.lamport,
            deps: Cow::Owned(Default::default()),
            len: 1,
        }
    }
}

#[inline(always)]
fn find_common_ancestor<'a, F, D>(
    get: &'a F,
    a_id: &Frontiers,
    b_id: &Frontiers,
) -> (Frontiers, DiffMode)
where
    D: DagNode + 'a,
    F: Fn(ID) -> Option<D>,
{
    if b_id.is_empty() {
        return (Default::default(), DiffMode::Checkout);
    }

    _find_common_ancestor_new(get, a_id, b_id)
}

/// - deep whether keep searching until the min of non-shared node is found
fn _find_common_ancestor<'a, F, D, G>(
    get: &'a F,
    a_ids: &Frontiers,
    b_ids: &Frontiers,
    notify: &mut G,
    find_path: bool,
) -> FxHashMap<PeerID, Counter>
where
    D: DagNode + 'a,
    F: Fn(ID) -> Option<D>,
    G: FnMut(IdSpan, NodeType),
{
    let mut ans: FxHashMap<PeerID, Counter> = Default::default();
    let mut queue: BinaryHeap<(OrdIdSpan, NodeType)> = BinaryHeap::new();
    for id in a_ids.iter() {
        queue.push((OrdIdSpan::from_dag_node(id, get).unwrap(), NodeType::A));
    }
    for id in b_ids.iter() {
        queue.push((OrdIdSpan::from_dag_node(id, get).unwrap(), NodeType::B));
    }
    let mut visited: HashMap<PeerID, (Counter, NodeType), _> = FxHashMap::default();
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
                        if visited.get(&node.id.peer).map(|(_, t)| *t) != Some(NodeType::Shared) {
                            ans.insert(node.id.peer, other_node.id_last().counter);
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
        if let Some((ctr, visited_type)) = visited.get_mut(&node.id.peer) {
            debug_assert!(*ctr >= node.id_last().counter);
            if *visited_type == NodeType::Shared {
                node_type = NodeType::Shared;
            } else if *visited_type != node_type {
                // if node_type is shared, ans should already contains it or its descendance
                if node_type != NodeType::Shared {
                    ans.insert(node.id.peer, node.id_last().counter);
                }
                *visited_type = NodeType::Shared;
                node_type = NodeType::Shared;
            }
        } else {
            visited.insert(node.id.peer, (node.id_last().counter, node_type));
        }

        // if this is not shared, the end of the span must be only reachable from A, or only reachable from B.
        // but the begin of the span may be reachable from both A and B
        notify(node.id_span(), node_type);

        match node_type {
            NodeType::A => a_count += node.deps.len(),
            NodeType::B => b_count += node.deps.len(),
            NodeType::Shared => {}
        }

        if a_count == 0 && b_count == 0 && (min.is_none() || &node <= min.as_ref().unwrap()) {
            if node_type != NodeType::Shared {
                ans.clear();
            }

            break;
        }

        for dep_id in node.deps.as_ref().iter() {
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
                        deps: Cow::Owned(Default::default()),
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

fn _find_common_ancestor_new<'a, F, D>(
    get: &'a F,
    left: &Frontiers,
    right: &Frontiers,
) -> (Frontiers, DiffMode)
where
    D: DagNode + 'a,
    F: Fn(ID) -> Option<D>,
{
    if right.is_empty() {
        return (Default::default(), DiffMode::Checkout);
    }

    if left.is_empty() {
        if right.len() == 1 {
            let mut node_id = right.as_single().unwrap();
            let mut node = get(node_id).unwrap();
            while node.deps().len() == 1 {
                node_id = node.deps().as_single().unwrap();
                node = get(node_id).unwrap();
            }

            if node.deps().is_empty() {
                return (Default::default(), DiffMode::Linear);
            }
        }

        return (Default::default(), DiffMode::ImportGreaterUpdates);
    }

    if left.len() == 1 && right.len() == 1 {
        let left = left.as_single().unwrap();
        let right = right.as_single().unwrap();
        if left.peer == right.peer {
            let left_span = get(left).unwrap();
            let right_span = get(right).unwrap();
            if left_span.id_start() == right_span.id_start() {
                if left.counter < right.counter {
                    return (left.into(), DiffMode::Linear);
                } else {
                    return (right.into(), DiffMode::Checkout);
                }
            }

            if left_span.deps().len() == 1
                && right_span.contains_id(left_span.deps().as_single().unwrap())
            {
                return (right.into(), DiffMode::Checkout);
            }

            if right_span.deps().len() == 1
                && left_span.contains_id(right_span.deps().as_single().unwrap())
            {
                return (left.into(), DiffMode::Linear);
            }
        }
    }

    let mut is_linear = left.len() <= 1 && right.len() == 1;
    let mut is_right_greater = true;
    let mut ans: Frontiers = Default::default();
    let mut queue: BinaryHeap<(SmallVec<[OrdIdSpan; 1]>, NodeType)> = BinaryHeap::new();

    fn ids_to_ord_id_spans<'a, D: DagNode + 'a, F: Fn(ID) -> Option<D>>(
        ids: &Frontiers,
        get: &'a F,
    ) -> Option<SmallVec<[OrdIdSpan<'a>; 1]>> {
        let mut ans: SmallVec<[OrdIdSpan<'a>; 1]> = SmallVec::with_capacity(ids.len());
        for id in ids.iter() {
            if let Some(node) = OrdIdSpan::from_dag_node(id, get) {
                ans.push(node);
            } else {
                return None;
            }
        }

        if ans.len() > 1 {
            ans.sort_unstable_by(|a, b| b.cmp(a));
        }

        Some(ans)
    }

    queue.push((ids_to_ord_id_spans(left, get).unwrap(), NodeType::A));
    queue.push((ids_to_ord_id_spans(right, get).unwrap(), NodeType::B));
    while let Some((mut node, mut node_type)) = queue.pop() {
        while let Some((other_node, other_type)) = queue.peek() {
            if node == *other_node
                || (node.len() == 1
                    && other_node.len() == 1
                    && node[0].id_last() == other_node[0].id_last())
            {
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

            // Iteration is done and no common ancestor is found
            // So the ans is empty
            is_right_greater = false;
            break;
        }

        // if node_type is A, then the left side is greater or parallel to the right side
        if node_type == NodeType::A {
            is_right_greater = false;
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

        if !node[0].deps.is_empty() {
            if let Some(deps) = ids_to_ord_id_spans(node[0].deps.as_ref(), get) {
                queue.push((deps, node_type));
            } else {
                // dep on trimmed history
                panic!("deps on trimmed history");
            }

            is_linear = false;
        } else {
            is_right_greater = false;
            break;
        }
    }

    let mode = if is_right_greater {
        if ans.len() <= 1 {
            debug_assert_eq!(&ans, left);
        }

        if is_linear {
            debug_assert!(ans.len() <= 1);
            DiffMode::Linear
        } else {
            DiffMode::ImportGreaterUpdates
        }
    } else {
        DiffMode::Checkout
    };

    (ans, mode)
}

pub fn remove_included_frontiers(frontiers: &mut VersionVector, new_change_deps: &[ID]) {
    for dep in new_change_deps.iter() {
        if let Some(last) = frontiers.get_last(dep.peer) {
            if last <= dep.counter {
                frontiers.remove(&dep.peer);
            }
        }
    }
}
