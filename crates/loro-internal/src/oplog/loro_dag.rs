use crate::change::{Change, Lamport};
use crate::dag::{Dag, DagNode};
use crate::id::{Counter, ID};
use crate::span::{HasId, HasLamport};
use crate::sync::Mutex;
use crate::version::{shrink_frontiers, Frontiers, ImVersionVector, VersionVector};
use loro_common::{HasCounter, HasCounterSpan, HasIdSpan, HasLamportSpan, PeerID};
use once_cell::sync::OnceCell;
use rle::{HasIndex, HasLength, Mergable, Sliceable};
use rustc_hash::FxHashSet;
use smallvec::SmallVec;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use std::fmt::Display;
use std::ops::{ControlFlow, Deref};
use std::sync::Arc;
use tracing::instrument;

use super::change_store::BatchDecodeInfo;
use super::ChangeStore;

/// [AppDag] maintains the causal graph of the app.
/// It's faster to answer the question like what's the LCA version
#[derive(Debug)]
pub struct AppDag {
    change_store: ChangeStore,
    /// It only contains nodes that are already parsed.
    ///
    /// - All the unparsed op ids must be included in `unparsed_vv`.
    /// - All the parsed and unparsed op ids must be included in `vv`.
    map: Mutex<BTreeMap<ID, AppDagNode>>,
    /// The latest known frontiers
    frontiers: Frontiers,
    /// The latest known version vector
    vv: VersionVector,
    /// The earliest known frontiers
    shallow_since_frontiers: Frontiers,
    /// The deps of the shallow frontiers
    shallow_root_frontiers_deps: Frontiers,
    /// The vv of shallow_frontiers_deps
    shallow_since_vv: ImVersionVector,
    /// Ops included in the version vector but not parsed yet
    ///
    /// # Invariants
    ///
    /// - `vv` >= `unparsed_vv`
    unparsed_vv: Mutex<VersionVector>,
    /// It's a set of points which are deps of some parsed ops.
    /// But the ops in this set are not parsed yet. When they are parsed,
    /// we need to make sure it breaks at the given point.
    unhandled_dep_points: Mutex<BTreeSet<ID>>,
    pending_txn_node: Option<AppDagNode>,
}

#[derive(Debug, Clone)]
pub struct AppDagNode {
    inner: Arc<AppDagNodeInner>,
}

impl Deref for AppDagNode {
    type Target = AppDagNodeInner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl AppDagNode {
    pub fn new(inner: AppDagNodeInner) -> Self {
        Self {
            inner: Arc::new(inner),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppDagNodeInner {
    pub(crate) peer: PeerID,
    pub(crate) cnt: Counter,
    pub(crate) lamport: Lamport,
    pub(crate) deps: Frontiers,
    pub(crate) vv: OnceCell<ImVersionVector>,
    /// A flag indicating whether any other nodes from a different peer depend on this node.
    /// The calculation of frontiers is based on the property that a node does not depend
    /// on the middle of other nodes.
    pub(crate) has_succ: bool,
    pub(crate) len: usize,
}

impl From<AppDagNodeInner> for AppDagNode {
    fn from(inner: AppDagNodeInner) -> Self {
        AppDagNode {
            inner: Arc::new(inner),
        }
    }
}

impl AppDag {
    pub(super) fn new(change_store: ChangeStore) -> Self {
        Self {
            change_store,
            map: Mutex::new(BTreeMap::new()),
            frontiers: Frontiers::default(),
            vv: VersionVector::default(),
            unparsed_vv: Mutex::new(VersionVector::default()),
            unhandled_dep_points: Mutex::new(BTreeSet::new()),
            shallow_since_frontiers: Default::default(),
            shallow_root_frontiers_deps: Default::default(),
            shallow_since_vv: Default::default(),
            pending_txn_node: None,
        }
    }

    pub fn frontiers(&self) -> &Frontiers {
        &self.frontiers
    }

    pub fn vv(&self) -> &VersionVector {
        &self.vv
    }

    pub fn shallow_since_vv(&self) -> &ImVersionVector {
        &self.shallow_since_vv
    }

    pub fn shallow_since_frontiers(&self) -> &Frontiers {
        &self.shallow_since_frontiers
    }

    pub fn is_empty(&self) -> bool {
        self.vv.is_empty()
    }

    #[tracing::instrument(skip_all, name = "handle_new_change")]
    pub(super) fn handle_new_change(&mut self, change: &Change, from_local: bool) {
        let len = change.content_len();
        self.update_version_on_new_change(change, from_local);
        #[cfg(debug_assertions)]
        {
            let unhandled_dep_points = self.unhandled_dep_points.lock().unwrap();
            let c = unhandled_dep_points
                .range(change.id_start()..change.id_end())
                .count();
            assert!(c == 0);
        }

        let mut inserted = false;
        if change.deps_on_self() {
            // We may not need to push new element to dag because it only depends on itself
            inserted = self.with_last_mut_of_peer(change.id.peer, |last| {
                let last = last.unwrap();
                if last.has_succ {
                    // Don't merge the node if there are other nodes depending on it
                    return false;
                }

                assert_eq!(last.peer, change.id.peer, "peer id is not the same");
                assert_eq!(
                    last.cnt + last.len as Counter,
                    change.id.counter,
                    "counter is not continuous"
                );
                assert_eq!(
                    last.lamport + last.len as Lamport,
                    change.lamport,
                    "lamport is not continuous"
                );
                let last = Arc::make_mut(&mut last.inner);
                last.len = (change.id.counter - last.cnt) as usize + len;
                last.has_succ = false;
                true
            });
        }

        if !inserted {
            let node: AppDagNode = AppDagNodeInner {
                vv: OnceCell::new(),
                peer: change.id.peer,
                cnt: change.id.counter,
                lamport: change.lamport,
                deps: change.deps.clone(),
                has_succ: false,
                len,
            }
            .into();

            let mut map = self.map.lock().unwrap();
            map.insert(node.id_start(), node);
            self.handle_deps_break_points(change.deps.iter(), change.id.peer, Some(&mut map));
        }
    }

    fn try_with_node_mut<R>(
        &self,
        map: &mut BTreeMap<ID, AppDagNode>,
        id: ID,
        f: impl FnOnce(Option<&mut AppDagNode>) -> R,
    ) -> R {
        let x = map.range_mut(..=id).next_back();
        if let Some((_, node)) = x {
            if node.contains_id(id) {
                f(Some(node))
            } else {
                f(None)
            }
        } else {
            f(None)
        }
    }

    /// If the lamport of change can be calculated, return Ok, otherwise, Err
    pub(crate) fn calc_unknown_lamport_change(&self, change: &mut Change) -> Result<(), ()> {
        for dep in change.deps.iter() {
            match self.get_lamport(&dep) {
                Some(lamport) => {
                    change.lamport = change.lamport.max(lamport + 1);
                }
                None => return Err(()),
            }
        }
        Ok(())
    }

    pub(crate) fn find_deps_of_id(&self, id: ID) -> Frontiers {
        let Some(node) = self.get(id) else {
            return Frontiers::default();
        };

        let offset = id.counter - node.cnt;
        if offset == 0 {
            node.deps.clone()
        } else {
            ID::new(id.peer, node.cnt + offset - 1).into()
        }
    }

    pub(crate) fn with_last_mut_of_peer<R>(
        &mut self,
        peer: PeerID,
        f: impl FnOnce(Option<&mut AppDagNode>) -> R,
    ) -> R {
        self.lazy_load_last_of_peer(peer);
        let mut binding = self.map.lock().unwrap();
        let last = binding
            .range_mut(..=ID::new(peer, Counter::MAX))
            .next_back()
            .map(|(_, v)| v);
        f(last)
    }

    fn update_version_on_new_change(&mut self, change: &Change, from_local: bool) {
        if from_local {
            assert!(self.pending_txn_node.take().is_some());
            assert_eq!(
                self.vv.get(&change.id.peer).copied().unwrap_or(0),
                change.ctr_end()
            );
        } else {
            let id_last = change.id_last();
            self.frontiers
                .update_frontiers_on_new_change(id_last, &change.deps);
            assert!(self.pending_txn_node.is_none());
            assert_eq!(
                self.vv.get(&change.id.peer).copied().unwrap_or(0),
                change.id.counter
            );
            self.vv.extend_to_include_last_id(id_last);
        }
    }

    pub(super) fn lazy_load_last_of_peer(&mut self, peer: u64) {
        let unparsed_vv = self.unparsed_vv.lock().unwrap();
        if !unparsed_vv.contains_key(&peer) || self.vv[&peer] >= unparsed_vv[&peer] {
            return;
        }

        let Some(nodes) = self.change_store.get_last_dag_nodes_for_peer(peer) else {
            panic!("unparsed vv don't match with change store. Peer:{peer} is not in change store")
        };

        self.lazy_load_nodes_internal(nodes, peer, None);
    }

    fn lazy_load_nodes_internal(
        &self,
        nodes: Vec<AppDagNode>,
        peer: u64,
        map_input: Option<&mut BTreeMap<ID, AppDagNode>>,
    ) {
        assert!(!nodes.is_empty());
        let mut map_guard = None;
        let map = map_input.unwrap_or_else(|| {
            map_guard = Some(self.map.lock().unwrap());
            map_guard.as_mut().unwrap()
        });
        let new_dag_start_counter_for_the_peer = nodes[0].cnt;
        let nodes_cnt_end = nodes.last().unwrap().ctr_end();
        let mut unparsed_vv = self.unparsed_vv.lock().unwrap();
        let end_counter = unparsed_vv[&peer];
        assert!(end_counter <= nodes_cnt_end);
        let mut deps_on_others = Vec::new();
        let mut break_point_set = self.unhandled_dep_points.lock().unwrap();
        for mut node in nodes {
            if node.cnt >= end_counter {
                // skip already parsed nodes
                break;
            }

            if node.cnt + node.len as Counter > end_counter {
                node = node.slice(0, (end_counter - node.cnt) as usize);
                // This is unlikely to happen
            }

            for dep in node.deps.iter() {
                if dep.peer != peer {
                    deps_on_others.push(dep);
                }
            }

            // PERF: we can try to merge the node with the previous node
            let break_point_ends: Vec<_> = break_point_set
                .range(node.id_start()..node.id_end())
                .map(|id| (id.counter - node.cnt) as usize + 1)
                .collect();
            if break_point_ends.is_empty() {
                map.insert(node.id_start(), node);
            } else {
                let mut slice_start = 0;
                for slice_end in break_point_ends.iter().copied() {
                    let mut slice_node = node.slice(slice_start, slice_end);
                    let inner = Arc::make_mut(&mut slice_node.inner);
                    inner.has_succ = true;
                    map.insert(slice_node.id_start(), slice_node);
                    slice_start = slice_end;
                }

                let last_break_point = break_point_ends.last().copied().unwrap();
                if last_break_point != node.len {
                    let slice_node = node.slice(last_break_point, node.len);
                    map.insert(slice_node.id_start(), slice_node);
                }

                for break_point in break_point_ends.into_iter() {
                    break_point_set.remove(&node.id_start().inc(break_point as Counter - 1));
                }
            }
        }

        if new_dag_start_counter_for_the_peer == 0 {
            unparsed_vv.remove(&peer);
        } else {
            unparsed_vv.insert(peer, new_dag_start_counter_for_the_peer);
        }
        drop(unparsed_vv);
        drop(break_point_set);
        self.handle_deps_break_points(deps_on_others.iter().copied(), peer, Some(map));
    }

    fn handle_deps_break_points(
        &self,
        ids: impl IntoIterator<Item = ID>,
        skip_peer: PeerID,
        map: Option<&mut BTreeMap<ID, AppDagNode>>,
    ) {
        let mut map_guard = None;
        let map = map.unwrap_or_else(|| {
            map_guard = Some(self.map.lock().unwrap());
            map_guard.as_mut().unwrap()
        });
        for id in ids {
            if id.peer == skip_peer {
                continue;
            }

            let mut handled = false;
            let ans = self.try_with_node_mut(map, id, |target| {
                // We don't need to break the dag node if it's not loaded yet
                let target = target?;
                if target.ctr_last() == id.counter {
                    let target = Arc::make_mut(&mut target.inner);
                    handled = true;
                    target.has_succ = true;
                    None
                } else {
                    // We need to split the target node into two part
                    // so that we can ensure the new change depends on the
                    // last id of a dag node.

                    let new_node =
                        target.slice(id.counter as usize - target.cnt as usize + 1, target.len);
                    let target = Arc::make_mut(&mut target.inner);
                    target.len -= new_node.len;
                    Some(new_node)
                }
            });

            if let Some(new_node) = ans {
                map.insert(new_node.id_start(), new_node);
            } else if !handled {
                self.unhandled_dep_points.lock().unwrap().insert(id);
            }
        }
    }

    fn ensure_lazy_load_node(&self, id: ID) {
        if self.shallow_since_vv.includes_id(id) {
            return;
        }

        loop {
            // We need to load all the dag nodes that has the same peer and greater counter than the given `id`
            // Because we only record the end counter of the unparsed version on `unparsed_vv`
            let unparsed_end = {
                let unparsed_vv = self.unparsed_vv.lock().unwrap();
                unparsed_vv.get(&id.peer).copied().unwrap_or(0)
            };
            if unparsed_end <= id.counter {
                return;
            }

            let last_unparsed_id = ID::new(id.peer, unparsed_end - 1);
            let Some(nodes) = self
                .change_store
                .get_dag_nodes_that_contains(last_unparsed_id)
            else {
                panic!("unparsed vv don't match with change store. Id:{id} is not in change store")
            };

            self.lazy_load_nodes_internal(nodes, id.peer, None);
        }
    }

    pub fn total_parsed_dag_node(&self) -> usize {
        self.map.lock().unwrap().len()
    }

    pub(crate) fn set_version_by_fast_snapshot_import(&mut self, v: BatchDecodeInfo) {
        assert!(self.vv.is_empty());
        *self.unparsed_vv.lock().unwrap() = v.vv.clone();
        self.vv = v.vv;
        self.frontiers = v.frontiers;
        if let Some((vv, f)) = v.start_version {
            if !f.is_empty() {
                assert!(f.len() == 1);
                let id = f.as_single().unwrap();
                let node = self.get(id).unwrap();
                assert!(node.cnt == id.counter);
                self.shallow_root_frontiers_deps = node.deps.clone();
            }
            self.shallow_since_frontiers = f;
            self.shallow_since_vv = ImVersionVector::from_vv(&vv);
        }
    }

    /// This method is slow and should only be used for debugging and testing.
    ///
    /// It will check the following properties:
    ///
    /// 1. Counter is continuous
    /// 2. A node always depends of the last ids of other nodes
    /// 3. Lamport is correctly calculated
    /// 4. VV for each node is correctly calculated
    /// 5. Frontiers are correctly calculated
    #[instrument(skip(self))]
    pub fn check_dag_correctness(&self) {
        {
            // parse all nodes
            let unparsed_vv = self.unparsed_vv.lock().unwrap().clone();
            for (peer, cnt) in unparsed_vv.iter() {
                if *cnt == 0 {
                    continue;
                }

                let mut end_cnt = *cnt;
                let init_counter = self.shallow_since_vv.get(peer).copied().unwrap_or(0);
                while end_cnt > init_counter {
                    let cnt = end_cnt - 1;
                    self.ensure_lazy_load_node(ID::new(*peer, cnt));
                    end_cnt = self
                        .unparsed_vv
                        .lock()
                        .unwrap()
                        .get(peer)
                        .copied()
                        .unwrap_or(0);
                }
            }

            self.unparsed_vv.lock().unwrap().clear();
        }
        {
            // check property 1: Counter is continuous
            let map = self.map.lock().unwrap();
            let mut last_end_id = ID::new(0, 0);
            for (&id, node) in map.iter() {
                let init_counter = self.shallow_since_vv.get(&id.peer).copied().unwrap_or(0);
                if id.peer == last_end_id.peer {
                    assert!(id.counter == last_end_id.counter);
                } else {
                    assert_eq!(id.counter, init_counter);
                }

                last_end_id = id.inc(node.len as Counter);
            }
        }
        {
            // check property 2: A node always depends of the last ids of other nodes
            let map = self.map.lock().unwrap();
            check_always_dep_on_last_id(&map);
        }
        {
            // check property 3: Lamport is correctly calculated
            let map = self.map.lock().unwrap();
            'outer: for (_, node) in map.iter() {
                let mut this_lamport = 0;
                for dep in node.deps.iter() {
                    if self.shallow_since_vv.includes_id(dep) {
                        continue 'outer;
                    }

                    let (_, dep_node) = map.range(..=dep).next_back().unwrap();
                    this_lamport = this_lamport.max(dep_node.lamport_end());
                }

                assert_eq!(this_lamport, node.lamport);
            }
        }
        {
            // check property 4: VV for each node is correctly calculated
            let map = self.map.lock().unwrap().clone();
            'outer: for (_, node) in map.iter() {
                let actual_vv = self.ensure_vv_for(node);
                let mut expected_vv = ImVersionVector::default();
                for dep in node.deps.iter() {
                    if self.shallow_since_vv.includes_id(dep) {
                        continue 'outer;
                    }

                    let (_, dep_node) = map.range(..=dep).next_back().unwrap();
                    self.ensure_vv_for(dep_node);
                    expected_vv.extend_to_include_vv(dep_node.vv.get().unwrap().iter());
                    expected_vv.extend_to_include_last_id(dep);
                }

                assert_eq!(actual_vv, expected_vv);
            }
        }
        {
            // check property 5: Frontiers are correctly calculated
            let mut maybe_frontiers = FxHashSet::default();
            let map = self.map.lock().unwrap();
            for (_, node) in map.iter() {
                maybe_frontiers.insert(node.id_last());
            }

            for (_, node) in map.iter() {
                for dep in node.deps.iter() {
                    maybe_frontiers.remove(&dep);
                }
            }

            let frontiers = self.frontiers.iter().collect::<FxHashSet<_>>();
            assert_eq!(maybe_frontiers, frontiers);
        }
    }

    pub(crate) fn can_export_shallow_snapshot_on(&self, deps: &Frontiers) -> bool {
        for id in deps.iter() {
            if !self.vv.includes_id(id) {
                return false;
            }
        }

        if self.is_before_shallow_root(deps) {
            return false;
        }

        true
    }

    pub(crate) fn is_before_shallow_root(&self, deps: &Frontiers) -> bool {
        // trace!("Is on shallow history? deps={:?}", deps);
        // trace!("self.shallow_since_vv {:?}", &self.shallow_since_vv);
        // trace!("self.shallow_frontiers {:?}", &self.shallow_since_frontiers);

        if self.shallow_since_vv.is_empty() {
            return false;
        }

        if deps.is_empty() {
            return true;
        }

        if deps.iter().any(|x| self.shallow_since_vv.includes_id(x)) {
            return true;
        }

        if deps
            .iter()
            .any(|x| self.shallow_since_frontiers.contains(&x))
        {
            return deps != &self.shallow_since_frontiers;
        }

        false
    }

    /// Travel the ancestors of the given id, and call the callback for each node
    ///
    /// It will travel the ancestors in the reverse order (from the greatest lamport to the smallest)
    pub(crate) fn travel_ancestors(
        &self,
        id: ID,
        f: &mut dyn FnMut(&AppDagNode) -> ControlFlow<()>,
    ) {
        struct PendingNode(AppDagNode);
        impl PartialEq for PendingNode {
            fn eq(&self, other: &Self) -> bool {
                self.0.lamport_last() == other.0.lamport_last() && self.0.peer == other.0.peer
            }
        }
        impl Eq for PendingNode {}
        impl PartialOrd for PendingNode {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }
        impl Ord for PendingNode {
            fn cmp(&self, other: &Self) -> Ordering {
                self.0
                    .lamport_last()
                    .cmp(&other.0.lamport_last())
                    .then_with(|| self.0.peer.cmp(&other.0.peer))
            }
        }

        let mut visited = FxHashSet::default();
        let mut pending: BinaryHeap<PendingNode> = BinaryHeap::new();
        pending.push(PendingNode(self.get(id).unwrap()));
        while let Some(PendingNode(node)) = pending.pop() {
            if f(&node).is_break() {
                break;
            }

            for dep in node.deps.iter() {
                let Some(dep_node) = self.get(dep) else {
                    continue;
                };
                if visited.contains(&dep_node.id_start()) {
                    continue;
                }

                visited.insert(dep_node.id_start());
                pending.push(PendingNode(dep_node));
            }
        }
    }

    pub(crate) fn update_version_on_new_local_op(
        &mut self,
        deps: &Frontiers,
        start_id: ID,
        start_lamport: Lamport,
        len: usize,
    ) {
        let last_id = start_id.inc(len as Counter - 1);
        // PERF: we can cache this last_id - this is a hot path
        self.vv.set_last(last_id);
        self.frontiers.update_frontiers_on_new_change(last_id, deps);
        match &mut self.pending_txn_node {
            Some(node) => {
                assert!(
                    node.peer == start_id.peer
                        && node.cnt + node.len as Counter == start_id.counter
                        && deps.len() == 1
                        && deps.as_single().unwrap().peer == start_id.peer
                );
                let inner = Arc::make_mut(&mut node.inner);
                inner.len += len;
            }
            None => {
                let node = AppDagNode {
                    inner: Arc::new(AppDagNodeInner {
                        peer: start_id.peer,
                        cnt: start_id.counter,
                        lamport: start_lamport,
                        deps: deps.clone(),
                        vv: OnceCell::new(),
                        has_succ: false,
                        len,
                    }),
                };
                self.pending_txn_node = Some(node);
            }
        }
    }

    pub(crate) fn latest_vv_contains_peer(&self, peer: PeerID) -> bool {
        self.vv.contains_key(&peer) && *self.vv.get(&peer).unwrap() > 0
    }
}

fn check_always_dep_on_last_id(map: &BTreeMap<ID, AppDagNode>) {
    for (_, node) in map.iter() {
        for dep in node.deps.iter() {
            let Some((&dep_id, dep_node)) = map.range(..=dep).next_back() else {
                // It's shallow
                continue;
            };
            assert_eq!(dep_node.id_start(), dep_id);
            if dep_node.contains_id(dep) {
                assert_eq!(dep_node.id_last(), dep);
            }
        }
    }
}

impl HasIndex for AppDagNode {
    type Int = Counter;
    fn get_start_index(&self) -> Self::Int {
        self.cnt
    }

    fn get_end_index(&self) -> Self::Int {
        self.cnt + self.len as Counter
    }
}

impl Sliceable for AppDagNode {
    fn slice(&self, from: usize, to: usize) -> Self {
        AppDagNodeInner {
            peer: self.peer,
            cnt: self.cnt + from as Counter,
            lamport: self.lamport + from as Lamport,
            deps: if from > 0 {
                Frontiers::from_id(self.id_start().inc(from as Counter - 1))
            } else {
                self.deps.clone()
            },
            vv: if let Some(vv) = self.vv.get() {
                let mut new = vv.clone();
                new.insert(self.peer, self.cnt + from as Counter);
                OnceCell::with_value(new)
            } else {
                OnceCell::new()
            },
            has_succ: if to == self.len { self.has_succ } else { true },
            len: to - from,
        }
        .into()
    }
}

impl HasId for AppDagNode {
    fn id_start(&self) -> ID {
        ID {
            peer: self.peer,
            counter: self.cnt,
        }
    }
}

impl HasCounter for AppDagNode {
    fn ctr_start(&self) -> Counter {
        self.cnt
    }
}

impl HasLength for AppDagNode {
    fn atom_len(&self) -> usize {
        self.len
    }

    fn content_len(&self) -> usize {
        self.len
    }
}

impl Mergable for AppDagNode {
    fn is_mergable(&self, other: &Self, _conf: &()) -> bool
    where
        Self: Sized,
    {
        !self.has_succ
            && self.peer == other.peer
            && self.cnt + self.len as Counter == other.cnt
            && other.deps.len() == 1
            && self.lamport + self.len as Lamport == other.lamport
            && other.deps.as_single().unwrap().peer == self.peer
    }

    fn merge(&mut self, other: &Self, _conf: &())
    where
        Self: Sized,
    {
        assert_eq!(
            other.deps.as_single().unwrap().counter,
            self.cnt + self.len as Counter - 1
        );
        let this = Arc::make_mut(&mut self.inner);
        this.len += other.len;
        this.has_succ = other.has_succ;
    }
}

impl HasLamport for AppDagNode {
    fn lamport(&self) -> Lamport {
        self.lamport
    }
}

impl DagNode for AppDagNode {
    fn deps(&self) -> &Frontiers {
        &self.deps
    }
}

impl Dag for AppDag {
    type Node = AppDagNode;

    fn frontier(&self) -> &Frontiers {
        &self.frontiers
    }

    fn get(&self, id: ID) -> Option<Self::Node> {
        self.ensure_lazy_load_node(id);
        let binding = self.map.lock().unwrap();
        if let Some(x) = binding.range(..=id).next_back() {
            if x.1.contains_id(id) {
                // PERF: do we need to optimize clone like this?
                // by adding another layer of Arc?
                return Some(x.1.clone());
            }
        }

        if let Some(node) = &self.pending_txn_node {
            if node.peer == id.peer && node.cnt <= id.counter {
                assert!(node.cnt + node.len as Counter > id.counter);
                return Some(node.clone());
            }
        }

        None
    }

    fn vv(&self) -> &VersionVector {
        &self.vv
    }

    fn contains(&self, id: ID) -> bool {
        self.vv.includes_id(id)
    }
}

impl AppDag {
    // PERF: this may be painfully slow
    /// get the version vector for a certain op.
    /// It's the version when the op is applied
    pub fn get_vv(&self, id: ID) -> Option<ImVersionVector> {
        self.get(id).map(|x| {
            let mut vv = self.ensure_vv_for(&x);
            vv.insert(id.peer, id.counter + 1);
            vv
        })
    }

    pub(crate) fn ensure_vv_for(&self, target_node: &AppDagNode) -> ImVersionVector {
        if target_node.vv.get().is_none() {
            // (node, has_processed_children)
            let mut stack: SmallVec<[AppDagNode; 4]> = smallvec::smallvec![target_node.clone()];
            while let Some(top_node) = stack.pop() {
                let mut ans_vv = ImVersionVector::default();
                // trace!("node={:?} {:?}", &top_node, has_all_deps_met);
                // trace!("deps={:?}", &top_node.deps);
                // trace!("this.shallow_f_deps={:?}", &self.shallow_frontiers_deps);
                // trace!("this.vv={:?}", &self.vv);
                // trace!("this.unparsed_vv={:?}", &self.unparsed_vv);
                // trace!("this.shallow_since_vv={:?}", &self.shallow_since_vv);
                if top_node.deps == self.shallow_root_frontiers_deps {
                    for (&p, &c) in self.shallow_since_vv.iter() {
                        ans_vv.insert(p, c);
                    }
                } else {
                    let mut all_deps_processed = true;
                    for id in top_node.deps.iter() {
                        let node = self.get(id).expect("deps should be in the dag");
                        if node.vv.get().is_none() {
                            // assert!(!has_all_deps_met);
                            if all_deps_processed {
                                stack.push(top_node.clone());
                            }
                            all_deps_processed = false;
                            stack.push(node);
                            continue;
                        };
                    }

                    if !all_deps_processed {
                        continue;
                    }

                    for id in top_node.deps.iter() {
                        let node = self.get(id).expect("deps should be in the dag");
                        let dep_vv = node.vv.get().unwrap();
                        if ans_vv.is_empty() {
                            ans_vv = dep_vv.clone();
                        } else {
                            ans_vv.extend_to_include_vv(dep_vv.iter());
                        }

                        ans_vv.insert(node.peer, node.ctr_end());
                    }
                }

                // trace!("ans_vv={:?}", &ans_vv);
                top_node.vv.set(ans_vv.clone()).unwrap();
            }
        }

        target_node.vv.get().unwrap().clone()
    }

    /// Compare the causal order of two versions.
    /// If None, two versions are concurrent to each other
    pub fn cmp_version(&self, a: ID, b: ID) -> Option<Ordering> {
        if a.peer == b.peer {
            return Some(a.counter.cmp(&b.counter));
        }

        let a = self.get_vv(a).unwrap();
        let b = self.get_vv(b).unwrap();
        a.partial_cmp(&b)
    }

    pub fn get_lamport(&self, id: &ID) -> Option<Lamport> {
        self.get(*id).and_then(|node| {
            assert!(id.counter >= node.cnt);
            if node.cnt + node.len as Counter > id.counter {
                Some(node.lamport + (id.counter - node.cnt) as Lamport)
            } else {
                None
            }
        })
    }

    pub fn get_change_lamport_from_deps(&self, deps: &Frontiers) -> Option<Lamport> {
        let mut lamport = 0;
        for id in deps.iter() {
            let x = self.get_lamport(&id)?;
            lamport = lamport.max(x + 1);
        }

        Some(lamport)
    }

    /// Convert a frontiers to a version vector
    ///
    /// If the frontiers version is not found in the dag, return None
    pub fn frontiers_to_vv(&self, frontiers: &Frontiers) -> Option<VersionVector> {
        if frontiers == &self.shallow_root_frontiers_deps {
            let vv = VersionVector::from_im_vv(&self.shallow_since_vv);
            return Some(vv);
        }

        let mut vv: VersionVector = Default::default();
        for id in frontiers.iter() {
            let x = self.get(id)?;
            let target_vv = self.ensure_vv_for(&x);
            vv.extend_to_include_vv(target_vv.iter());
            vv.extend_to_include_last_id(id);
        }

        Some(vv)
    }

    #[allow(unused)]
    pub(crate) fn frontiers_to_im_vv(&self, frontiers: &Frontiers) -> ImVersionVector {
        if frontiers.is_empty() {
            return Default::default();
        }

        let mut iter = frontiers.iter();
        let mut vv = {
            let id = iter.next().unwrap();
            let Some(x) = self.get(id) else {
                unreachable!()
            };
            let mut vv = self.ensure_vv_for(&x);
            vv.extend_to_include_last_id(id);
            vv
        };

        for id in iter {
            let Some(x) = self.get(id) else {
                unreachable!()
            };
            let x = self.ensure_vv_for(&x);
            vv.extend_to_include_vv(x.iter());
            vv.extend_to_include_last_id(id);
        }

        vv
    }

    pub fn im_vv_to_frontiers(&self, vv: &ImVersionVector) -> Frontiers {
        if vv.is_empty() {
            return Default::default();
        }

        let this = vv;
        let last_ids: Frontiers = this
            .iter()
            .filter_map(|(client_id, cnt)| {
                if *cnt == 0 {
                    return None;
                }

                if self
                    .shallow_since_vv
                    .includes_id(ID::new(*client_id, *cnt - 1))
                {
                    return None;
                }

                Some(ID::new(*client_id, cnt - 1))
            })
            .collect();

        if last_ids.is_empty() {
            return self.shallow_since_frontiers.clone();
        }

        shrink_frontiers(&last_ids, self).unwrap()
    }

    pub fn vv_to_frontiers(&self, vv: &VersionVector) -> Frontiers {
        if vv.is_empty() {
            return Default::default();
        }

        let this = vv;
        let last_ids: Frontiers = this
            .iter()
            .filter_map(|(client_id, cnt)| {
                if *cnt == 0 {
                    return None;
                }

                if self
                    .shallow_since_vv
                    .includes_id(ID::new(*client_id, *cnt - 1))
                {
                    return None;
                }

                Some(ID::new(*client_id, cnt - 1))
            })
            .collect();

        if last_ids.is_empty() {
            return self.shallow_since_frontiers.clone();
        }

        shrink_frontiers(&last_ids, self).unwrap()
    }

    pub(crate) fn frontiers_to_next_lamport(&self, frontiers: &Frontiers) -> Lamport {
        if frontiers.is_empty() {
            return 0;
        }

        let mut iter = frontiers.iter();
        let mut lamport = {
            let id = iter.next().unwrap();
            let Some(x) = self.get(id) else {
                unreachable!()
            };
            assert!(id.counter >= x.cnt);
            (id.counter - x.cnt) as Lamport + x.lamport + 1
        };

        for id in iter {
            let Some(x) = self.get(id) else {
                unreachable!()
            };
            assert!(id.counter >= x.cnt);
            lamport = lamport.max((id.counter - x.cnt) as Lamport + x.lamport + 1);
        }

        lamport
    }

    pub fn get_frontiers(&self) -> &Frontiers {
        &self.frontiers
    }

    /// - Ordering::Less means self is less than target or parallel
    /// - Ordering::Equal means versions equal
    /// - Ordering::Greater means self's version is greater than target
    pub fn cmp_with_frontiers(&self, other: &Frontiers) -> Ordering {
        if &self.frontiers == other {
            Ordering::Equal
        } else if other.iter().all(|id| self.vv.includes_id(id)) {
            Ordering::Greater
        } else {
            Ordering::Less
        }
    }

    // PERF
    /// Compare two [Frontiers] causally.
    ///
    /// If one of the [Frontiers] are not included, it will return [FrontiersNotIncluded].
    pub fn cmp_frontiers(
        &self,
        a: &Frontiers,
        b: &Frontiers,
    ) -> Result<Option<Ordering>, FrontiersNotIncluded> {
        let a = self.frontiers_to_vv(a).ok_or(FrontiersNotIncluded)?;
        let b = self.frontiers_to_vv(b).ok_or(FrontiersNotIncluded)?;
        Ok(a.partial_cmp(&b))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct FrontiersNotIncluded;
impl Display for FrontiersNotIncluded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("The given Frontiers are not included by the doc")
    }
}
