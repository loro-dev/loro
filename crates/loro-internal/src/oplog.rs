mod change_store;
pub(crate) mod dag;
mod iter;
mod pending_changes;

use once_cell::sync::OnceCell;
use std::borrow::Cow;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Mutex;

use crate::change::{get_sys_timestamp, Change, Lamport, Timestamp};
use crate::configure::Configure;
use crate::container::list::list_op;
use crate::dag::{Dag, DagUtils};
use crate::diff_calc::DiffMode;
use crate::encoding::ParsedHeaderAndBody;
use crate::encoding::{decode_oplog, encode_oplog, EncodeMode};
use crate::history_cache::ContainerHistoryCache;
use crate::id::{Counter, PeerID, ID};
use crate::op::{FutureInnerContent, ListSlice, RawOpContent, RemoteOp, RichOp};
use crate::span::{HasCounterSpan, HasIdSpan, HasLamportSpan};
use crate::version::{Frontiers, ImVersionVector, VersionVector};
use crate::LoroError;
use change_store::BlockOpRef;
pub use change_store::{BlockChangeRef, ChangeStore};
use loro_common::{HasId, IdLp, IdSpan};
use rle::{HasLength, Mergable, RleVec, Sliceable};
use smallvec::SmallVec;

pub use self::dag::FrontiersNotIncluded;
use self::iter::MergedChangeIter;
use self::pending_changes::PendingChanges;

use super::arena::SharedArena;

/// [OpLog] store all the ops i.e. the history.
/// It allows multiple [AppState] to attach to it.
/// So you can derive different versions of the state from the [OpLog].
/// It allows us to build a version control system.
///
/// The causal graph should always be a DAG and complete. So we can always find the LCA.
/// If deps are missing, we can't import the change. It will be put into the `pending_changes`.
pub struct OpLog {
    pub(crate) dag: AppDag,
    pub(crate) arena: SharedArena,
    change_store: ChangeStore,
    history_cache: Mutex<ContainerHistoryCache>,
    /// **lamport starts from 0**
    pub(crate) next_lamport: Lamport,
    pub(crate) latest_timestamp: Timestamp,
    /// Pending changes that haven't been applied to the dag.
    /// A change can be imported only when all its deps are already imported.
    /// Key is the ID of the missing dep
    pub(crate) pending_changes: PendingChanges,
    /// Whether we are importing a batch of changes.
    /// If so the Dag's frontiers won't be updated until the batch is finished.
    pub(crate) batch_importing: bool,
    pub(crate) configure: Configure,
}

/// [AppDag] maintains the causal graph of the app.
/// It's faster to answer the question like what's the LCA version
#[derive(Debug, Clone, Default)]
pub struct AppDag {
    pub(crate) map: BTreeMap<ID, AppDagNode>,
    pub(crate) frontiers: Frontiers,
    pub(crate) vv: VersionVector,
}

#[derive(Debug, Clone)]
pub struct AppDagNode {
    pub(crate) peer: PeerID,
    pub(crate) cnt: Counter,
    pub(crate) lamport: Lamport,
    pub(crate) deps: Frontiers,
    pub(crate) vv: OnceCell<ImVersionVector>,
    /// A flag indicating whether any other nodes depend on this node.
    /// The calculation of frontiers is based on this value.
    pub(crate) has_succ: bool,
    pub(crate) len: usize,
}

impl OpLog {
    pub(crate) fn fork(&self, arena: SharedArena, configure: Configure) -> Self {
        let change_store = self
            .change_store
            .fork(arena.clone(), configure.merge_interval.clone());
        Self {
            change_store: change_store.clone(),
            dag: self.dag.clone(),
            arena: self.arena.clone(),
            history_cache: Mutex::new(
                self.history_cache
                    .lock()
                    .unwrap()
                    .fork(arena.clone(), change_store),
            ),
            next_lamport: self.next_lamport,
            latest_timestamp: self.latest_timestamp,
            pending_changes: Default::default(),
            batch_importing: false,
            configure,
        }
    }
}

impl AppDag {
    pub fn get_mut(&mut self, id: ID) -> Option<&mut AppDagNode> {
        let x = self.map.range_mut(..=id).next_back()?;
        if x.1.contains_id(id) {
            Some(x.1)
        } else {
            None
        }
    }

    pub(crate) fn refresh_frontiers(&mut self) {
        let vv_iter = self.vv.iter();
        // PERF:
        self.frontiers = vv_iter
            .filter_map(|(peer, _)| {
                let node = self.get_last_of_peer(*peer)?;
                if node.has_succ {
                    return None;
                }

                Some(ID::new(*peer, node.ctr_last()))
            })
            .collect();
        // dbg!(&self);
    }

    /// If the lamport of change can be calculated, return Ok, otherwise, Err
    pub(crate) fn calc_unknown_lamport_change(&self, change: &mut Change) -> Result<(), ()> {
        for dep in change.deps.iter() {
            match self.get_lamport(dep) {
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

    pub(crate) fn get_last_of_peer(&self, peer: PeerID) -> Option<&AppDagNode> {
        self.map
            .range(..=ID::new(peer, Counter::MAX))
            .next_back()
            .map(|(_, v)| v)
    }

    pub(crate) fn get_last_mut_of_peer(&mut self, peer: PeerID) -> Option<&mut AppDagNode> {
        self.map
            .range_mut(..=ID::new(peer, Counter::MAX))
            .next_back()
            .map(|(_, v)| v)
    }
}

impl std::fmt::Debug for OpLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpLog")
            .field("dag", &self.dag)
            .field("pending_changes", &self.pending_changes)
            .field("next_lamport", &self.next_lamport)
            .field("latest_timestamp", &self.latest_timestamp)
            .finish()
    }
}

pub(crate) struct EnsureDagNodeDepsAreAtTheEnd;

impl OpLog {
    #[inline]
    pub(crate) fn new() -> Self {
        let arena = SharedArena::new();
        let cfg = Configure::default();
        let change_store = ChangeStore::new_mem(&arena, cfg.merge_interval.clone());
        Self {
            history_cache: Mutex::new(ContainerHistoryCache::new(
                arena.clone(),
                change_store.clone(),
            )),
            change_store,
            dag: AppDag::default(),
            arena,
            next_lamport: 0,
            latest_timestamp: Timestamp::default(),
            pending_changes: Default::default(),
            batch_importing: false,
            configure: cfg,
        }
    }

    #[inline]
    pub fn latest_timestamp(&self) -> Timestamp {
        self.latest_timestamp
    }

    #[inline]
    pub fn dag(&self) -> &AppDag {
        &self.dag
    }

    pub fn change_store(&self) -> &ChangeStore {
        &self.change_store
    }

    /// Get the change with the given peer and lamport.
    ///
    /// If not found, return the change with the greatest lamport that is smaller than the given lamport.
    pub fn get_change_with_lamport(
        &self,
        peer: PeerID,
        lamport: Lamport,
    ) -> Option<BlockChangeRef> {
        self.change_store
            .get_change_by_idlp(IdLp::new(peer, lamport))
    }

    pub fn get_timestamp_of_version(&self, f: &Frontiers) -> Timestamp {
        let mut timestamp = Timestamp::default();
        for id in f.iter() {
            if let Some(change) = self.lookup_change(*id) {
                timestamp = timestamp.max(change.timestamp);
            }
        }

        timestamp
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.dag.map.is_empty() && self.arena.can_import_snapshot()
    }

    /// This is the **only** place to update the `OpLog.changes`
    pub(crate) fn insert_new_change(&mut self, change: Change, _: EnsureDagNodeDepsAreAtTheEnd) {
        self.history_cache
            .lock()
            .unwrap()
            .insert_by_new_change(&change, true, true);
        self.change_store.insert_change(change.clone());
        self.register_container_and_parent_link(&change);
    }

    #[inline(always)]
    pub(crate) fn with_history_cache<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut ContainerHistoryCache) -> R,
    {
        let mut history_cache = self.history_cache.lock().unwrap();
        f(&mut history_cache)
    }

    pub fn has_history_cache(&self) -> bool {
        self.history_cache.lock().unwrap().has_cache()
    }

    pub fn free_history_cache(&self) {
        let mut history_cache = self.history_cache.lock().unwrap();
        history_cache.free();
    }

    /// Import a change.
    ///
    /// Pending changes that haven't been applied to the dag.
    /// A change can be imported only when all its deps are already imported.
    /// Key is the ID of the missing dep
    ///
    /// # Err
    ///
    /// - Return Err(LoroError::UsedOpID) when the change's id is occupied
    /// - Return Err(LoroError::DecodeError) when the change's deps are missing
    pub(crate) fn import_local_change(&mut self, change: Change) -> Result<(), LoroError> {
        let Some(change) = self.trim_the_known_part_of_change(change) else {
            return Ok(());
        };
        self.check_id_is_not_duplicated(change.id)?;
        if let Err(id) = self.check_deps(&change.deps) {
            return Err(LoroError::DecodeError(
                format!("Missing dep {:?}", id).into_boxed_str(),
            ));
        }

        if cfg!(debug_assertions) {
            let lamport = self.dag.frontiers_to_next_lamport(&change.deps);
            assert_eq!(
                lamport, change.lamport,
                "{:#?}\nDAG={:#?}",
                &change, &self.dag
            );
        }

        self.next_lamport = self.next_lamport.max(change.lamport_end());
        self.latest_timestamp = self.latest_timestamp.max(change.timestamp);
        self.dag.vv.extend_to_include_last_id(change.id_last());
        self.dag.frontiers.retain_non_included(&change.deps);
        self.dag.frontiers.filter_peer(change.id.peer);
        self.dag.frontiers.push(change.id_last());
        let mark = self.update_dag_on_new_change(&change);
        self.insert_new_change(change, mark);
        Ok(())
    }

    /// Every time we import a new change, it should run this function to update the dag
    pub(crate) fn update_dag_on_new_change(
        &mut self,
        change: &Change,
    ) -> EnsureDagNodeDepsAreAtTheEnd {
        let len = change.content_len();
        if change.deps_on_self() {
            // don't need to push new element to dag because it only depends on itself
            let last = self.dag.get_last_mut_of_peer(change.id.peer).unwrap();
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
            last.len = (change.id.counter - last.cnt) as usize + len;
            last.has_succ = false;
        } else {
            let vv = self.dag.frontiers_to_im_vv(&change.deps);
            let mut pushed = false;
            let node = AppDagNode {
                vv: OnceCell::from(vv),
                peer: change.id.peer,
                cnt: change.id.counter,
                lamport: change.lamport,
                deps: change.deps.clone(),
                has_succ: false,
                len,
            };
            if let Some(last) = self.dag.get_last_mut_of_peer(change.id.peer) {
                if change.id.counter > 0 {
                    assert_eq!(
                        last.ctr_end(),
                        change.id.counter,
                        "counter is not continuous"
                    );
                }

                if last.is_mergable(&node, &()) {
                    pushed = true;
                    last.merge(&node, &());
                }
            }

            if !pushed {
                self.dag.map.insert(node.id_start(), node);
            }

            for dep in change.deps.iter() {
                let target = self.dag.get_mut(*dep).unwrap();
                if target.ctr_last() == dep.counter {
                    target.has_succ = true;
                } else {
                    // We need to split the target node into two part
                    // so that we can ensure the new change depends on the
                    // last id of a dag node.
                    let new_node =
                        target.slice(dep.counter as usize - target.cnt as usize, target.len);
                    target.len -= new_node.len;
                    self.dag.map.insert(new_node.id_start(), new_node);
                }
            }
        }

        EnsureDagNodeDepsAreAtTheEnd
    }

    /// Trim the known part of change
    pub(crate) fn trim_the_known_part_of_change(&self, change: Change) -> Option<Change> {
        let Some(&end) = self.dag.vv.get(&change.id.peer) else {
            return Some(change);
        };

        if change.id.counter >= end {
            return Some(change);
        }

        if change.ctr_end() <= end {
            return None;
        }

        let offset = (end - change.id.counter) as usize;
        Some(change.slice(offset, change.atom_len()))
    }

    fn check_id_is_not_duplicated(&self, id: ID) -> Result<(), LoroError> {
        let cur_end = self.dag.vv.get(&id.peer).cloned().unwrap_or(0);
        if cur_end > id.counter {
            return Err(LoroError::UsedOpID { id });
        }

        Ok(())
    }

    fn check_deps(&self, deps: &Frontiers) -> Result<(), ID> {
        for dep in deps.iter() {
            if !self.dag.vv.includes_id(*dep) {
                return Err(*dep);
            }
        }

        Ok(())
    }

    pub(crate) fn next_id(&self, peer: PeerID) -> ID {
        let cnt = self.dag.vv.get(&peer).copied().unwrap_or(0);
        ID::new(peer, cnt)
    }

    pub(crate) fn vv(&self) -> &VersionVector {
        &self.dag.vv
    }

    pub(crate) fn frontiers(&self) -> &Frontiers {
        &self.dag.frontiers
    }

    /// - Ordering::Less means self is less than target or parallel
    /// - Ordering::Equal means versions equal
    /// - Ordering::Greater means self's version is greater than target
    pub fn cmp_with_frontiers(&self, other: &Frontiers) -> Ordering {
        self.dag.cmp_with_frontiers(other)
    }

    /// Compare two [Frontiers] causally.
    ///
    /// If one of the [Frontiers] are not included, it will return [FrontiersNotIncluded].
    #[inline]
    pub fn cmp_frontiers(
        &self,
        a: &Frontiers,
        b: &Frontiers,
    ) -> Result<Option<Ordering>, FrontiersNotIncluded> {
        self.dag.cmp_frontiers(a, b)
    }

    pub(crate) fn get_min_lamport_at(&self, id: ID) -> Lamport {
        self.get_change_at(id).map(|c| c.lamport).unwrap_or(0)
    }

    pub(crate) fn get_lamport_at(&self, id: ID) -> Option<Lamport> {
        self.get_change_at(id)
            .map(|c| c.lamport + (id.counter - c.id.counter) as Lamport)
    }

    pub(crate) fn iter_ops(&self, id_span: IdSpan) -> impl Iterator<Item = RichOp<'static>> + '_ {
        let change_iter = self.change_store.iter_changes(id_span);
        change_iter.flat_map(move |c| RichOp::new_iter_by_cnt_range(c, id_span.counter))
    }

    pub(crate) fn get_max_lamport_at(&self, id: ID) -> Lamport {
        self.get_change_at(id)
            .map(|c| {
                let change_counter = c.id.counter as u32;
                c.lamport + c.ops().last().map(|op| op.counter).unwrap_or(0) as u32 - change_counter
            })
            .unwrap_or(Lamport::MAX)
    }

    pub fn get_change_at(&self, id: ID) -> Option<BlockChangeRef> {
        self.change_store.get_change(id)
    }

    pub fn get_deps_of(&self, id: ID) -> Option<Frontiers> {
        self.get_change_at(id).map(|c| {
            if c.id.counter == id.counter {
                c.deps.clone()
            } else {
                Frontiers::from_id(id.inc(-1))
            }
        })
    }

    pub fn get_remote_change_at(&self, id: ID) -> Option<Change<RemoteOp<'static>>> {
        let change = self.get_change_at(id)?;
        Some(convert_change_to_remote(&self.arena, &change))
    }

    pub(crate) fn import_unknown_lamport_pending_changes(
        &mut self,
        remote_changes: Vec<Change>,
    ) -> Result<(), LoroError> {
        let latest_vv = self.dag.vv.clone();
        self.extend_pending_changes_with_unknown_lamport(remote_changes, &latest_vv);
        Ok(())
    }

    /// lookup change by id.
    ///
    /// if id does not included in this oplog, return None
    pub(crate) fn lookup_change(&self, id: ID) -> Option<BlockChangeRef> {
        self.change_store.get_change(id)
    }

    #[inline(always)]
    pub(crate) fn export_from(&self, vv: &VersionVector) -> Vec<u8> {
        encode_oplog(self, vv, EncodeMode::Auto)
    }

    #[inline(always)]
    pub(crate) fn decode(&mut self, data: ParsedHeaderAndBody) -> Result<(), LoroError> {
        decode_oplog(self, data)
    }

    /// Iterates over all changes between `a` and `b` peer by peer (not in causal order, fast)
    pub(crate) fn for_each_change_within(
        &self,
        a: &VersionVector,
        b: &VersionVector,
        mut f: impl FnMut(&Change),
    ) {
        let spans = b.iter_between(a);
        for span in spans {
            for c in self.change_store.iter_changes(span) {
                f(&c);
            }
        }
    }

    /// iterates over all changes between LCA(common ancestors) to the merged version of (`from` and `to`) causally
    ///
    /// Tht iterator will include a version vector when the change is applied
    ///
    /// returns: (common_ancestor_vv, iterator)
    ///
    /// Note: the change returned by the iterator may include redundant ops at the beginning, you should trim it by yourself.
    /// You can trim it by the provided counter value. It should start with the counter.
    ///
    /// If frontiers are provided, it will be faster (because we don't need to calculate it from version vector
    pub(crate) fn iter_from_lca_causally(
        &self,
        from: &VersionVector,
        from_frontiers: Option<&Frontiers>,
        to: &VersionVector,
        to_frontiers: Option<&Frontiers>,
    ) -> (
        VersionVector,
        DiffMode,
        impl Iterator<
                Item = (
                    BlockChangeRef,
                    (Counter, Counter),
                    Rc<RefCell<VersionVector>>,
                ),
            > + '_,
    ) {
        let mut merged_vv = from.clone();
        merged_vv.merge(to);
        let from_frontiers_inner;
        let to_frontiers_inner;

        let from_frontiers = match from_frontiers {
            Some(f) => f,
            None => {
                from_frontiers_inner = Some(from.to_frontiers(&self.dag));
                from_frontiers_inner.as_ref().unwrap()
            }
        };

        let to_frontiers = match to_frontiers {
            Some(t) => t,
            None => {
                to_frontiers_inner = Some(to.to_frontiers(&self.dag));
                to_frontiers_inner.as_ref().unwrap()
            }
        };

        let (common_ancestors, mut diff_mode) =
            self.dag.find_common_ancestor(from_frontiers, to_frontiers);
        if diff_mode == DiffMode::Checkout && to > from {
            diff_mode = DiffMode::Import;
        }

        let common_ancestors_vv = self.dag.frontiers_to_vv(&common_ancestors).unwrap();
        // go from lca to merged_vv
        let diff = common_ancestors_vv.diff(&merged_vv).right;
        let mut iter = self.dag.iter_causal(&common_ancestors, diff);
        let mut node = iter.next();
        let mut cur_cnt = 0;
        let vv = Rc::new(RefCell::new(VersionVector::default()));
        (
            common_ancestors_vv.clone(),
            diff_mode,
            std::iter::from_fn(move || {
                if let Some(inner) = &node {
                    let mut inner_vv = vv.borrow_mut();
                    // FIXME: PERF: it looks slow for large vv, like 10000+ entries
                    inner_vv.clear();
                    inner_vv.extend_to_include_vv(inner.data.vv.get().unwrap().iter());
                    let peer = inner.data.peer;
                    let cnt = inner
                        .data
                        .cnt
                        .max(cur_cnt)
                        .max(common_ancestors_vv.get(&peer).copied().unwrap_or(0));
                    let dag_node_end = (inner.data.cnt + inner.data.len as Counter)
                        .min(merged_vv.get(&peer).copied().unwrap_or(0));
                    let change = self.change_store.get_change(ID::new(peer, cnt)).unwrap();

                    if change.ctr_end() < dag_node_end {
                        cur_cnt = change.ctr_end();
                    } else {
                        node = iter.next();
                        cur_cnt = 0;
                    }

                    inner_vv.extend_to_include_end_id(change.id);

                    Some((change, (cnt, dag_node_end), vv.clone()))
                } else {
                    None
                }
            }),
        )
    }

    pub fn len_changes(&self) -> usize {
        self.change_store.change_num()
    }

    pub fn diagnose_size(&self) -> SizeInfo {
        let mut total_changes = 0;
        let mut total_ops = 0;
        let mut total_atom_ops = 0;
        let total_dag_node = self.dag.map.len();
        self.change_store.visit_all_changes(&mut |change| {
            total_changes += 1;
            total_ops += change.ops.len();
            total_atom_ops += change.atom_len();
        });

        println!("total changes: {}", total_changes);
        println!("total ops: {}", total_ops);
        println!("total atom ops: {}", total_atom_ops);
        println!("total dag node: {}", total_dag_node);
        SizeInfo {
            total_changes,
            total_ops,
            total_atom_ops,
            total_dag_node,
        }
    }

    pub(crate) fn iter_changes_peer_by_peer<'a>(
        &'a self,
        from: &VersionVector,
        to: &VersionVector,
    ) -> impl Iterator<Item = BlockChangeRef> + 'a {
        let spans: Vec<_> = from.diff_iter(to).1.collect();
        spans
            .into_iter()
            .flat_map(move |span| self.change_store.iter_changes(span))
    }

    pub(crate) fn iter_changes_causally_rev<'a>(
        &'a self,
        from: &VersionVector,
        to: &VersionVector,
    ) -> impl Iterator<Item = BlockChangeRef> + 'a {
        MergedChangeIter::new_change_iter_rev(self, from, to)
    }

    pub fn get_timestamp_for_next_txn(&self) -> Timestamp {
        if self.configure.record_timestamp() {
            get_sys_timestamp()
        } else {
            0
        }
    }

    pub(crate) fn idlp_to_id(&self, id: loro_common::IdLp) -> Option<ID> {
        let change = self.change_store.get_change_by_idlp(id)?;
        if change.lamport > id.lamport || change.lamport_end() <= id.lamport {
            return None;
        }

        Some(ID::new(
            change.id.peer,
            (id.lamport - change.lamport) as Counter + change.id.counter,
        ))
    }

    #[allow(unused)]
    pub(crate) fn id_to_idlp(&self, id_start: ID) -> IdLp {
        let change = self.get_change_at(id_start).unwrap();
        let lamport = change.lamport + (id_start.counter - change.id.counter) as Lamport;
        let peer = id_start.peer;
        loro_common::IdLp { peer, lamport }
    }

    pub(crate) fn get_op(&self, id: ID) -> Option<BlockOpRef> {
        let change = self.get_change_at(id)?;
        change.get_op_with_counter(id.counter)
    }

    pub(crate) fn split_span_based_on_deps(&self, id_span: IdSpan) -> Vec<(IdSpan, Frontiers)> {
        let peer = id_span.peer;
        let mut counter = id_span.counter.min();
        let span_end = id_span.counter.norm_end();
        let mut ans = Vec::new();

        while counter < span_end {
            let id = ID::new(peer, counter);
            let node = self.dag.get(id).unwrap();

            let f = if node.cnt == counter {
                node.deps.clone()
            } else if counter > 0 {
                id.inc(-1).into()
            } else {
                unreachable!()
            };

            let cur_end = node.cnt + node.len as Counter;
            let len = cur_end.min(span_end) - counter;
            ans.push((id.to_span(len as usize), f));
            counter += len;
        }

        ans
    }

    #[inline]
    pub fn compact_change_store(&mut self) {
        self.change_store.flush_and_compact();
    }
}

#[derive(Debug)]
pub struct SizeInfo {
    pub total_changes: usize,
    pub total_ops: usize,
    pub total_atom_ops: usize,
    pub total_dag_node: usize,
}

pub(crate) fn convert_change_to_remote(
    arena: &SharedArena,
    change: &Change,
) -> Change<RemoteOp<'static>> {
    let mut ops = RleVec::new();
    for op in change.ops.iter() {
        for op in local_op_to_remote(arena, op) {
            ops.push(op);
        }
    }

    Change {
        ops,
        id: change.id,
        deps: change.deps.clone(),
        lamport: change.lamport,
        timestamp: change.timestamp,
    }
}

pub(crate) fn local_op_to_remote(
    arena: &SharedArena,
    op: &crate::op::Op,
) -> SmallVec<[RemoteOp<'static>; 1]> {
    let container = arena.get_container_id(op.container).unwrap();
    let mut contents: SmallVec<[_; 1]> = SmallVec::new();
    match &op.content {
        crate::op::InnerContent::List(list) => match list {
            list_op::InnerListOp::Insert { slice, pos } => match container.container_type() {
                loro_common::ContainerType::Text => {
                    let str = arena
                        .slice_str_by_unicode_range(slice.0.start as usize..slice.0.end as usize);
                    contents.push(RawOpContent::List(list_op::ListOp::Insert {
                        slice: ListSlice::RawStr {
                            unicode_len: str.chars().count(),
                            str: Cow::Owned(str),
                        },
                        pos: *pos,
                    }));
                }
                loro_common::ContainerType::List | loro_common::ContainerType::MovableList => {
                    contents.push(RawOpContent::List(list_op::ListOp::Insert {
                        slice: ListSlice::RawData(Cow::Owned(
                            arena.get_values(slice.0.start as usize..slice.0.end as usize),
                        )),
                        pos: *pos,
                    }))
                }
                _ => unreachable!(),
            },
            list_op::InnerListOp::InsertText {
                slice,
                unicode_len: len,
                unicode_start: _,
                pos,
            } => match container.container_type() {
                loro_common::ContainerType::Text => {
                    contents.push(RawOpContent::List(list_op::ListOp::Insert {
                        slice: ListSlice::RawStr {
                            unicode_len: *len as usize,
                            str: Cow::Owned(std::str::from_utf8(slice).unwrap().to_owned()),
                        },
                        pos: *pos as usize,
                    }));
                }
                _ => unreachable!(),
            },
            list_op::InnerListOp::Delete(del) => {
                contents.push(RawOpContent::List(list_op::ListOp::Delete(*del)))
            }
            list_op::InnerListOp::StyleStart {
                start,
                end,
                key,
                value,
                info,
            } => contents.push(RawOpContent::List(list_op::ListOp::StyleStart {
                start: *start,
                end: *end,
                key: key.clone(),
                value: value.clone(),
                info: *info,
            })),
            list_op::InnerListOp::StyleEnd => {
                contents.push(RawOpContent::List(list_op::ListOp::StyleEnd))
            }
            list_op::InnerListOp::Move {
                from,
                elem_id: from_id,
                to,
            } => contents.push(RawOpContent::List(list_op::ListOp::Move {
                from: *from,
                elem_id: *from_id,
                to: *to,
            })),
            list_op::InnerListOp::Set { elem_id, value } => {
                contents.push(RawOpContent::List(list_op::ListOp::Set {
                    elem_id: *elem_id,
                    value: value.clone(),
                }))
            }
        },
        crate::op::InnerContent::Map(map) => {
            let value = map.value.clone();
            contents.push(RawOpContent::Map(crate::container::map::MapSet {
                key: map.key.clone(),
                value,
            }))
        }
        crate::op::InnerContent::Tree(tree) => contents.push(RawOpContent::Tree(tree.clone())),
        crate::op::InnerContent::Future(f) => match f {
            #[cfg(feature = "counter")]
            crate::op::FutureInnerContent::Counter(c) => contents.push(RawOpContent::Counter(*c)),
            FutureInnerContent::Unknown { prop, value } => {
                contents.push(crate::op::RawOpContent::Unknown {
                    prop: *prop,
                    value: value.clone(),
                })
            }
        },
    };

    let mut ans = SmallVec::with_capacity(contents.len());
    for content in contents {
        ans.push(RemoteOp {
            container: container.clone(),
            content,
            counter: op.counter,
        })
    }
    ans
}
