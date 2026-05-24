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

use loro_common::IdSpanVector;
use rle::{HasLength, Sliceable};
use rustc_hash::{FxHashMap, FxHashSet};
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
        vv.try_update_last(node_id);
        if !visited.contains(&node_id_start) {
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
                let Some(next) = get(node_id) else {
                    return (Default::default(), DiffMode::ImportGreaterUpdates);
                };
                node = next;
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
    let mut has_unmatched_branch = false;
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

    fn push_ord_id_spans<'a>(
        queue: &mut BinaryHeap<(SmallVec<[OrdIdSpan<'a>; 1]>, NodeType)>,
        spans: SmallVec<[OrdIdSpan<'a>; 1]>,
        node_type: NodeType,
    ) {
        for span in spans {
            queue.push((smallvec::smallvec![span], node_type));
        }
    }

    fn deps_to_ord_id_spans<'a, D: DagNode + 'a, F: Fn(ID) -> Option<D>>(
        node: &OrdIdSpan<'a>,
        get: &'a F,
    ) -> Option<SmallVec<[OrdIdSpan<'a>; 1]>> {
        let mut deps = ids_to_ord_id_spans(node.deps.as_ref(), get)?;
        if node.id.counter > 0 {
            let prev = node.id.inc(-1);
            if let Some(prev) = OrdIdSpan::from_dag_node(prev, get) {
                if !deps.iter().any(|dep| dep.contains_id(prev.id_last())) {
                    deps.push(prev);
                }
            }
        }

        if deps.len() > 1 {
            deps.sort_unstable_by(|a, b| b.cmp(a));
        }

        Some(deps)
    }

    fn shrink_ancestor_frontiers<'a, D: DagNode + 'a, F: Fn(ID) -> Option<D>>(
        ids: &Frontiers,
        get: &'a F,
    ) -> Frontiers {
        if ids.len() <= 1 {
            return ids.clone();
        }

        let mut ids = ids_to_ord_id_spans(ids, get).expect("common ancestors should be in dag");
        ids.sort_unstable();
        let mut frontiers = Vec::with_capacity(ids.len());
        for id in ids.iter().rev() {
            let mut should_insert = true;
            for frontier in frontiers.iter().rev() {
                if contains_in_ancestors(get, *frontier, id) {
                    should_insert = false;
                    break;
                }
            }

            if should_insert {
                frontiers.push(id.id_last());
            }
        }

        frontiers.into_iter().collect()
    }

    fn has_trimmed_history_deps<'a, D: DagNode + 'a, F: Fn(ID) -> Option<D>>(
        ids: &Frontiers,
        get: &'a F,
    ) -> bool {
        ids.iter().any(|id| {
            let Some(node) = OrdIdSpan::from_dag_node(id, get) else {
                return true;
            };
            ids_to_ord_id_spans(node.deps.as_ref(), get).is_none()
        })
    }

    fn contains_in_ancestors<'a, D: DagNode + 'a, F: Fn(ID) -> Option<D>>(
        get: &'a F,
        frontier: ID,
        target: &OrdIdSpan<'_>,
    ) -> bool {
        let mut visited = FxHashSet::default();
        let mut pending = BinaryHeap::new();
        let Some(node) = OrdIdSpan::from_dag_node(frontier, get) else {
            return false;
        };
        pending.push(node);
        while let Some(node) = pending.pop() {
            if node.contains_id(target.id_last()) {
                return true;
            }

            if node.lamport_last() < target.lamport_last() {
                break;
            }

            if !visited.insert(node.id_start()) {
                continue;
            }

            if let Some(deps) = deps_to_ord_id_spans(&node, get) {
                for dep in deps {
                    pending.push(dep);
                }
            }
        }

        false
    }

    push_ord_id_spans(
        &mut queue,
        ids_to_ord_id_spans(left, get).unwrap(),
        NodeType::A,
    );
    push_ord_id_spans(
        &mut queue,
        ids_to_ord_id_spans(right, get).unwrap(),
        NodeType::B,
    );
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

        if node_type == NodeType::Shared {
            for id in node.iter().map(|x| x.id_last()) {
                ans.push(id);
            }
            continue;
        }

        if queue.is_empty() {
            has_unmatched_branch = true;
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
                node[0].len = if other.0[0].lamport_last() >= node[0].lamport {
                    (other.0[0].lamport_last() - node[0].lamport + 1).min(node[0].len as u32 - 1)
                        as usize
                } else {
                    1
                };
                queue.push((node, node_type));
                continue;
            }
        }

        if let Some(deps) = deps_to_ord_id_spans(&node[0], get) {
            if !deps.is_empty() {
                push_ord_id_spans(&mut queue, deps, node_type);
                is_linear = false;
                continue;
            }
        } else {
            // The dependency is on trimmed shallow history. The exact ancestor is
            // not representable in the current DAG, so fall back to a conservative
            // checkout base.
            has_unmatched_branch = true;
            is_right_greater = false;
            continue;
        }

        {
            // Some checkout calculators still require replaying from a base that
            // includes every branch whose operation positions may affect the diff.
            // In non-linear checkout mode, an earlier common ancestor is a valid
            // conservative base even when it is not the mathematical LCA.
            has_unmatched_branch = true;
            is_right_greater = false;
            continue;
        }
    }

    ans = shrink_ancestor_frontiers(&ans, get);
    if has_unmatched_branch && !has_trimmed_history_deps(&ans, get) {
        ans = Default::default();
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use loro_common::{HasId, HasIdSpan};
    use rand::{rngs::StdRng, Rng, SeedableRng};

    use super::*;

    #[derive(Clone, Debug)]
    struct TestNode {
        id: ID,
        len: usize,
        lamport: Lamport,
        deps: Frontiers,
    }

    impl DagNode for TestNode {
        fn deps(&self) -> &Frontiers {
            &self.deps
        }
    }

    impl HasId for TestNode {
        fn id_start(&self) -> ID {
            self.id
        }
    }

    impl HasLamport for TestNode {
        fn lamport(&self) -> Lamport {
            self.lamport
        }
    }

    impl HasLength for TestNode {
        fn content_len(&self) -> usize {
            self.len
        }
    }

    impl Sliceable for TestNode {
        fn slice(&self, from: usize, to: usize) -> Self {
            Self {
                id: self.id.inc(from as i32),
                len: to - from,
                lamport: self.lamport + from as Lamport,
                deps: if from == 0 {
                    self.deps.clone()
                } else {
                    self.id.inc(from as i32 - 1).into()
                },
            }
        }
    }

    #[derive(Debug)]
    struct TestDag {
        nodes: BTreeMap<ID, TestNode>,
        vv: VersionVector,
        frontier: Frontiers,
    }

    impl TestDag {
        fn new(nodes: impl IntoIterator<Item = TestNode>, frontier: Frontiers) -> Self {
            let mut vv = VersionVector::default();
            let nodes = nodes
                .into_iter()
                .map(|node| {
                    vv.set_end(node.id_end());
                    (node.id_start(), node)
                })
                .collect();
            Self {
                nodes,
                vv,
                frontier,
            }
        }
    }

    impl Dag for TestDag {
        type Node = TestNode;

        fn get(&self, id: ID) -> Option<Self::Node> {
            self.nodes
                .range(..=id)
                .rev()
                .find(|(_, node)| node.contains_id(id))
                .map(|(_, node)| node.clone())
        }

        fn frontier(&self) -> &Frontiers {
            &self.frontier
        }

        fn vv(&self) -> &VersionVector {
            &self.vv
        }

        fn contains(&self, id: ID) -> bool {
            self.get(id).is_some()
        }
    }

    fn node(
        peer: PeerID,
        counter: Counter,
        len: usize,
        lamport: Lamport,
        deps: Frontiers,
    ) -> TestNode {
        TestNode {
            id: ID::new(peer, counter),
            len,
            lamport,
            deps,
        }
    }

    fn ancestors_of_frontiers(dag: &TestDag, frontiers: &Frontiers) -> FxHashSet<ID> {
        let mut ans = FxHashSet::default();
        for id in frontiers.iter() {
            collect_ancestors(dag, id, &mut ans);
        }
        ans
    }

    fn collect_ancestors(dag: &TestDag, id: ID, ans: &mut FxHashSet<ID>) {
        let mut stack = vec![id];
        let mut visited_targets = FxHashSet::default();
        while let Some(id) = stack.pop() {
            if !visited_targets.insert(id) {
                continue;
            }

            for node in dag.nodes.values() {
                if node.id_start().peer != id.peer || node.id_start().counter > id.counter {
                    continue;
                }

                let end = node.id_last().counter.min(id.counter);
                for counter in node.id_start().counter..=end {
                    ans.insert(ID::new(node.id_start().peer, counter));
                }

                for dep in node.deps().iter() {
                    stack.push(dep);
                }
            }
        }
    }

    fn is_ancestor(dag: &TestDag, ancestor: ID, descendant: ID) -> bool {
        let mut ancestors = FxHashSet::default();
        collect_ancestors(dag, descendant, &mut ancestors);
        ancestors.contains(&ancestor)
    }

    fn maximal_frontiers(dag: &TestDag, ids: impl IntoIterator<Item = ID>) -> Frontiers {
        let mut ids = Vec::<ID>::from_iter(ids);
        ids.sort_by_key(|id| dag.get(*id).unwrap().get_lamport_from_counter(id.counter));
        let mut frontiers = Vec::new();
        for id in ids.into_iter().rev() {
            if frontiers
                .iter()
                .any(|frontier| is_ancestor(dag, id, *frontier))
            {
                continue;
            }

            frontiers.retain(|frontier| !is_ancestor(dag, *frontier, id));
            frontiers.push(id);
        }

        frontiers.into_iter().collect()
    }

    fn oracle_common_ancestor(dag: &TestDag, left: &Frontiers, right: &Frontiers) -> Frontiers {
        let left_ancestors = ancestors_of_frontiers(dag, left);
        let right_ancestors = ancestors_of_frontiers(dag, right);
        maximal_frontiers(
            dag,
            left_ancestors
                .into_iter()
                .filter(|id| right_ancestors.contains(id)),
        )
    }

    fn assert_common_ancestor_valid_against_oracle(
        dag: &TestDag,
        left: &Frontiers,
        right: &Frontiers,
    ) {
        let (actual, mode) = dag.find_common_ancestor(left, right);
        let expected = oracle_common_ancestor(dag, left, right);
        let left_ancestors = ancestors_of_frontiers(dag, left);
        let right_ancestors = ancestors_of_frontiers(dag, right);
        for id in actual.iter() {
            assert!(
                left_ancestors.contains(&id) && right_ancestors.contains(&id),
                "actual LCA id {id} must be common: left={left:?} right={right:?} actual={actual:?} expected={expected:?} mode={mode:?}\ndag={dag:?}",
            );
        }

        for a in actual.iter() {
            for b in actual.iter() {
                if a != b {
                    assert!(
                        !is_ancestor(dag, a, b),
                        "actual LCA must be a minimal frontier set: left={left:?} right={right:?} actual={actual:?} expected={expected:?} mode={mode:?}\ndag={dag:?}",
                    );
                }
            }
        }

        if !matches!(mode, DiffMode::Checkout) {
            assert_eq!(
                actual, expected,
                "non-checkout mode should use the maximal common ancestor: left={left:?} right={right:?} mode={mode:?}\ndag={dag:?}",
            );
            assert_eq!(
                &actual, left,
                "non-checkout mode must use left as LCA: left={left:?} right={right:?} mode={mode:?}"
            );
            for id in left.iter() {
                assert!(
                    right_ancestors.contains(&id),
                    "non-checkout mode requires right to include left id {id}; left={left:?} right={right:?} mode={mode:?}",
                );
            }
        }
    }

    fn all_ids(dag: &TestDag) -> Vec<ID> {
        dag.nodes
            .values()
            .flat_map(|node| {
                (0..node.content_len()).map(|offset| node.id_start().inc(offset as Counter))
            })
            .collect()
    }

    fn random_frontiers(dag: &TestDag, ids: &[ID], rng: &mut impl Rng) -> Frontiers {
        if ids.is_empty() || rng.gen_bool(0.1) {
            return Frontiers::default();
        }

        let len = rng.gen_range(1..=ids.len().min(4));
        maximal_frontiers(dag, (0..len).map(|_| ids[rng.gen_range(0..ids.len())]))
    }

    fn random_dag(seed: u64, node_count: usize) -> TestDag {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut next_counter = [0 as Counter; 8];
        let mut nodes = Vec::new();
        let mut existing_ids = Vec::new();
        for i in 0..node_count {
            let peer = rng.gen_range(1..next_counter.len() as PeerID);
            let counter = next_counter[peer as usize];
            let len = rng.gen_range(1..=3);
            next_counter[peer as usize] += len as Counter;
            let dep_count = if existing_ids.is_empty() {
                0
            } else {
                rng.gen_range(0..=existing_ids.len().min(3))
            };
            let deps = maximal_frontiers(
                &TestDag::new(nodes.clone(), Frontiers::default()),
                (0..dep_count).map(|_| existing_ids[rng.gen_range(0..existing_ids.len())]),
            );
            let node = node(peer, counter, len, i as Lamport * 4, deps);
            existing_ids.extend((0..len).map(|offset| node.id_start().inc(offset as Counter)));
            nodes.push(node);
        }

        let dag = TestDag::new(nodes, Frontiers::default());
        let frontier = maximal_frontiers(&dag, existing_ids);
        TestDag::new(dag.nodes.into_values(), frontier)
    }

    fn layered_merge_dag() -> TestDag {
        let root = node(1, 0, 3, 0, Frontiers::default());
        let left = node(1, 3, 2, 4, ID::new(1, 2).into());
        let right = node(2, 0, 3, 5, ID::new(1, 1).into());
        let late_right = node(2, 3, 2, 9, ID::new(2, 2).into());
        let third = node(3, 0, 2, 6, ID::new(1, 2).into());
        let merge_left_right = node(
            4,
            0,
            1,
            12,
            Frontiers::from([left.id_last(), right.id_last()]),
        );
        let merge_all = node(
            5,
            0,
            2,
            16,
            Frontiers::from([
                merge_left_right.id_last(),
                late_right.id_last(),
                third.id_last(),
            ]),
        );
        let independent = node(6, 0, 2, 20, Frontiers::default());
        let final_merge = node(
            7,
            0,
            1,
            25,
            Frontiers::from([merge_all.id_last(), independent.id_last()]),
        );

        TestDag::new(
            vec![
                root,
                left,
                right,
                late_right,
                third,
                merge_left_right,
                merge_all,
                independent,
                final_merge.clone(),
            ],
            final_merge.id_last().into(),
        )
    }

    #[test]
    fn common_ancestor_handles_empty_linear_same_span_and_parent_child_cases() {
        let first = node(1, 0, 2, 0, Frontiers::default());
        let second = node(1, 2, 2, 2, ID::new(1, 1).into());
        let dag = TestDag::new(vec![first, second], ID::new(1, 3).into());

        assert_eq!(
            dag.find_common_ancestor(&Frontiers::default(), &ID::new(1, 3).into()),
            (Frontiers::default(), DiffMode::Linear)
        );
        assert_eq!(
            dag.find_common_ancestor(&ID::new(1, 3).into(), &Frontiers::default()),
            (Frontiers::default(), DiffMode::Checkout)
        );
        assert_eq!(
            dag.find_common_ancestor(&ID::new(1, 0).into(), &ID::new(1, 1).into()),
            (ID::new(1, 0).into(), DiffMode::Linear)
        );
        assert_eq!(
            dag.find_common_ancestor(&ID::new(1, 1).into(), &ID::new(1, 0).into()),
            (ID::new(1, 0).into(), DiffMode::Checkout)
        );
        assert_eq!(
            dag.find_common_ancestor(&ID::new(1, 1).into(), &ID::new(1, 3).into()),
            (ID::new(1, 1).into(), DiffMode::Linear)
        );
    }

    #[test]
    fn common_ancestor_left_empty_stops_linear_scan_at_missing_shallow_dependency() {
        let visible = node(1, 1, 1, 1, ID::new(1, 0).into());
        let dag = TestDag::new(vec![visible], ID::new(1, 1).into());

        assert_eq!(
            dag.find_common_ancestor(&Frontiers::default(), &ID::new(1, 1).into()),
            (Frontiers::default(), DiffMode::ImportGreaterUpdates)
        );
    }

    #[test]
    fn common_ancestor_of_parallel_branches_is_shared_dependency() {
        let root = node(1, 0, 1, 0, Frontiers::default());
        let left = node(2, 0, 1, 1, root.id.into());
        let right = node(3, 0, 1, 2, root.id.into());
        let merge = node(4, 0, 1, 3, Frontiers::from([left.id, right.id]));
        let dag = TestDag::new(
            vec![root.clone(), left.clone(), right.clone(), merge.clone()],
            merge.id.into(),
        );

        let (ancestor, mode) = dag.find_common_ancestor(&left.id.into(), &right.id.into());
        assert_eq!(ancestor, root.id.into());
        assert_eq!(mode, DiffMode::Checkout);

        let (ancestor, _) = dag.find_common_ancestor(&root.id.into(), &merge.id.into());
        assert_eq!(ancestor, root.id.into());
    }

    #[test]
    fn common_ancestor_falls_back_before_independent_branch() {
        let left = node(1, 0, 1, 0, Frontiers::default());
        let independent = node(2, 0, 1, 1, Frontiers::default());
        let merge = node(3, 0, 1, 2, Frontiers::from([left.id, independent.id]));
        let dag = TestDag::new(
            vec![left.clone(), independent, merge.clone()],
            merge.id.into(),
        );

        let (ancestor, mode) = dag.find_common_ancestor(&left.id.into(), &merge.id.into());
        assert_eq!(ancestor, Frontiers::default());
        assert_eq!(mode, DiffMode::Checkout);
    }

    #[test]
    fn common_ancestor_falls_back_before_unmatched_branch_with_multiple_left_frontiers() {
        let left_a = node(1, 0, 1, 0, Frontiers::default());
        let left_b = node(2, 0, 1, 1, Frontiers::default());
        let independent = node(3, 0, 1, 2, Frontiers::default());
        let left_frontiers = Frontiers::from([left_a.id, left_b.id]);
        let merge = node(
            4,
            0,
            1,
            3,
            Frontiers::from([left_a.id, left_b.id, independent.id]),
        );
        let dag = TestDag::new(
            vec![left_a.clone(), left_b.clone(), independent, merge.clone()],
            merge.id.into(),
        );

        let (ancestor, mode) = dag.find_common_ancestor(&left_frontiers, &merge.id.into());
        assert_eq!(ancestor, Frontiers::default());
        assert_eq!(mode, DiffMode::Checkout);
    }

    #[test]
    fn common_ancestor_marks_cross_peer_direct_dependency_as_greater_update() {
        let left = node(1, 0, 1, 0, Frontiers::default());
        let right = node(2, 0, 1, 1, left.id.into());
        let dag = TestDag::new(vec![left.clone(), right.clone()], right.id.into());

        let (ancestor, mode) = dag.find_common_ancestor(&left.id.into(), &right.id.into());
        assert_eq!(ancestor, left.id.into());
        assert_eq!(mode, DiffMode::ImportGreaterUpdates);
    }

    #[test]
    fn common_ancestor_falls_back_when_right_adds_concurrent_branch_from_shared_root() {
        let root = node(1, 0, 1, 0, Frontiers::default());
        let left = node(2, 0, 1, 1, root.id.into());
        let concurrent = node(3, 0, 1, 2, root.id.into());
        let merge = node(4, 0, 1, 3, Frontiers::from([left.id, concurrent.id]));
        let dag = TestDag::new(
            vec![root, left.clone(), concurrent, merge.clone()],
            merge.id.into(),
        );

        let (ancestor, mode) = dag.find_common_ancestor(&left.id.into(), &merge.id.into());
        assert_eq!(ancestor, Frontiers::default());
        assert_eq!(mode, DiffMode::Checkout);
    }

    #[test]
    fn common_ancestor_keeps_target_when_checking_out_to_ancestor_with_extra_branch() {
        let root = node(1, 0, 1, 0, Frontiers::default());
        let left = node(2, 0, 2, 1, root.id.into());
        let right = node(3, 0, 2, 3, root.id.into());
        let extra = node(
            4,
            0,
            1,
            5,
            Frontiers::from([left.id_last(), right.id_last()]),
        );
        let dag = TestDag::new(
            vec![root, left.clone(), right.clone(), extra.clone()],
            extra.id.into(),
        );
        let target = Frontiers::from([ID::new(2, 0), ID::new(3, 0)]);
        let current = Frontiers::from([extra.id, left.id_last(), right.id_last()]);

        let (ancestor, mode) = dag.find_common_ancestor(&current, &target);
        assert_eq!(ancestor, target);
        assert_eq!(mode, DiffMode::Checkout);
    }

    #[test]
    fn common_ancestor_does_not_keep_ancestor_of_shared_descendant() {
        let root = node(1, 0, 1, 0, Frontiers::default());
        let shared = node(2, 0, 1, 1, root.id.into());
        let left_only = node(3, 0, 1, 2, root.id.into());
        let right_only = node(4, 0, 1, 3, root.id.into());
        let dag = TestDag::new(
            vec![root, shared.clone(), left_only.clone(), right_only.clone()],
            Frontiers::from([shared.id, left_only.id, right_only.id]),
        );

        let (ancestor, mode) = dag.find_common_ancestor(
            &Frontiers::from([shared.id, left_only.id]),
            &Frontiers::from([shared.id, right_only.id]),
        );
        assert_eq!(ancestor, shared.id.into());
        assert_eq!(mode, DiffMode::Checkout);
    }

    #[test]
    fn common_ancestor_valid_against_slow_oracle_on_random_dags() {
        for seed in 0..128 {
            let mut rng = StdRng::seed_from_u64(seed);
            let dag = random_dag(seed, rng.gen_range(1..=18));
            let ids = all_ids(&dag);
            for _ in 0..64 {
                let left = random_frontiers(&dag, &ids, &mut rng);
                let right = random_frontiers(&dag, &ids, &mut rng);
                assert_common_ancestor_valid_against_oracle(&dag, &left, &right);
            }
        }
    }

    #[test]
    fn common_ancestor_valid_against_slow_oracle_on_all_pairs_in_small_random_dags() {
        for seed in 1000..1020 {
            let dag = random_dag(seed, 8);
            let ids = all_ids(&dag);
            let mut frontiers = vec![Frontiers::default()];
            frontiers.extend(ids.iter().copied().map(Frontiers::from));
            for chunk in ids.chunks(3) {
                frontiers.push(maximal_frontiers(&dag, chunk.iter().copied()));
            }

            for left in frontiers.iter() {
                for right in frontiers.iter() {
                    assert_common_ancestor_valid_against_oracle(&dag, left, right);
                }
            }
        }
    }

    #[test]
    fn common_ancestor_valid_against_slow_oracle_on_layered_merge_dag() {
        let dag = layered_merge_dag();
        let ids = all_ids(&dag);
        let mut frontiers = vec![Frontiers::default(), dag.frontier().clone()];
        frontiers.extend(ids.iter().copied().map(Frontiers::from));
        frontiers.extend([
            Frontiers::from([ID::new(1, 4), ID::new(2, 2)]),
            Frontiers::from([ID::new(1, 4), ID::new(3, 1)]),
            Frontiers::from([ID::new(4, 0), ID::new(2, 4), ID::new(3, 1)]),
            Frontiers::from([ID::new(5, 1), ID::new(6, 1)]),
        ]);

        for left in frontiers.iter() {
            for right in frontiers.iter() {
                assert_common_ancestor_valid_against_oracle(&dag, left, right);
            }
        }
    }

    #[test]
    fn common_ancestor_valid_against_slow_oracle_on_branchy_random_dags() {
        for seed in 2000..2064 {
            let mut rng = StdRng::seed_from_u64(seed);
            let dag = random_dag(seed, rng.gen_range(20..=36));
            let ids = all_ids(&dag);
            for _ in 0..96 {
                let left = random_frontiers(&dag, &ids, &mut rng);
                let right = random_frontiers(&dag, &ids, &mut rng);
                assert_common_ancestor_valid_against_oracle(&dag, &left, &right);
            }
        }
    }

    #[test]
    fn find_path_reports_forward_and_retreat_spans_for_linear_and_branch_paths() {
        let first = node(1, 0, 2, 0, Frontiers::default());
        let second = node(1, 2, 2, 2, ID::new(1, 1).into());
        let side = node(2, 0, 1, 1, ID::new(1, 1).into());
        let dag = TestDag::new(
            vec![first, second, side],
            Frontiers::from([ID::new(1, 3), ID::new(2, 0)]),
        );

        let forward = dag.find_path(&ID::new(1, 0).into(), &ID::new(1, 3).into());
        assert!(forward.retreat.is_empty());
        assert_eq!(forward.forward.get(&1), Some(&CounterSpan::new(1, 4)));

        let retreat = dag.find_path(&ID::new(1, 3).into(), &ID::new(1, 0).into());
        assert!(retreat.forward.is_empty());
        assert_eq!(retreat.retreat.get(&1), Some(&CounterSpan::new(1, 4)));

        let branch = dag.find_path(&ID::new(2, 0).into(), &ID::new(1, 3).into());
        assert_eq!(branch.retreat.get(&2), Some(&CounterSpan::new(0, 1)));
        assert_eq!(branch.forward.get(&1), Some(&CounterSpan::new(2, 4)));
    }

    #[test]
    fn get_version_vector_walks_dependencies_once_and_tracks_dep_ids_inside_spans() {
        let base = node(1, 0, 3, 0, Frontiers::default());
        let left = node(2, 0, 1, 4, ID::new(1, 1).into());
        let right = node(3, 0, 1, 5, ID::new(1, 2).into());
        let merge = node(4, 0, 1, 6, Frontiers::from([left.id, right.id]));
        let dag = TestDag::new(vec![base, left, right, merge.clone()], merge.id.into());

        let vv = dag.get_vv(merge.id);
        assert_eq!(vv.get_last(1), Some(2));
        assert_eq!(vv.get_last(2), Some(0));
        assert_eq!(vv.get_last(3), Some(0));
        assert_eq!(vv.get_last(4), Some(0));
    }

    #[test]
    fn remove_included_frontiers_drops_only_dependencies_at_or_after_recorded_frontiers() {
        let mut vv = VersionVector::default();
        vv.set_last(ID::new(1, 3));
        vv.set_last(ID::new(2, 4));
        vv.set_last(ID::new(3, 1));

        remove_included_frontiers(&mut vv, &[ID::new(1, 3), ID::new(2, 2), ID::new(4, 0)]);

        assert_eq!(vv.get_last(1), None);
        assert_eq!(vv.get_last(2), Some(4));
        assert_eq!(vv.get_last(3), Some(1));
    }

    #[test]
    fn ord_id_span_orders_by_last_lamport_peer_and_shorter_overlapping_span() {
        let short = OrdIdSpan {
            id: ID::new(1, 1),
            lamport: 1,
            len: 1,
            deps: Cow::Owned(Frontiers::default()),
        };
        let long = OrdIdSpan {
            id: ID::new(1, 0),
            lamport: 0,
            len: 2,
            deps: Cow::Owned(Frontiers::default()),
        };
        let later_peer = OrdIdSpan {
            id: ID::new(2, 0),
            lamport: 1,
            len: 1,
            deps: Cow::Owned(Frontiers::default()),
        };

        assert!(short > long);
        assert!(later_peer > short);
        assert_eq!(long.get_min().len, 1);
    }
}
