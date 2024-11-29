mod change_store;
pub(crate) mod loro_dag;
mod pending_changes;

use bytes::Bytes;
use std::borrow::Cow;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;
use std::sync::Mutex;
use tracing::{debug, trace, trace_span};

use self::change_store::iter::MergedChangeIter;
use self::pending_changes::PendingChanges;
use super::arena::SharedArena;
use crate::change::{get_sys_timestamp, Change, Lamport, Timestamp};
use crate::configure::Configure;
use crate::container::list::list_op;
use crate::dag::{Dag, DagUtils};
use crate::diff_calc::DiffMode;
use crate::encoding::{decode_oplog, encode_oplog, EncodeMode};
use crate::encoding::{ImportStatus, ParsedHeaderAndBody};
use crate::history_cache::ContainerHistoryCache;
use crate::id::{Counter, PeerID, ID};
use crate::op::{FutureInnerContent, ListSlice, RawOpContent, RemoteOp, RichOp};
use crate::span::{HasCounterSpan, HasLamportSpan};
use crate::version::{Frontiers, ImVersionVector, VersionVector};
use crate::LoroError;
use change_store::BlockOpRef;
use loro_common::{IdLp, IdSpan};
use rle::{HasLength, RleVec, Sliceable};
use smallvec::SmallVec;

pub use self::loro_dag::{AppDag, AppDagNode, FrontiersNotIncluded};
pub use change_store::{BlockChangeRef, ChangeStore};

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
    /// Pending changes that haven't been applied to the dag.
    /// A change can be imported only when all its deps are already imported.
    /// Key is the ID of the missing dep
    pub(crate) pending_changes: PendingChanges,
    /// Whether we are importing a batch of changes.
    /// If so the Dag's frontiers won't be updated until the batch is finished.
    pub(crate) batch_importing: bool,
    pub(crate) configure: Configure,
}

impl std::fmt::Debug for OpLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpLog")
            .field("dag", &self.dag)
            .field("pending_changes", &self.pending_changes)
            .finish()
    }
}

impl OpLog {
    #[inline]
    pub(crate) fn new() -> Self {
        let arena = SharedArena::new();
        let cfg = Configure::default();
        let change_store = ChangeStore::new_mem(&arena, cfg.merge_interval.clone());
        Self {
            history_cache: Mutex::new(ContainerHistoryCache::new(change_store.clone(), None)),
            dag: AppDag::new(change_store.clone()),
            change_store,
            arena,
            pending_changes: Default::default(),
            batch_importing: false,
            configure: cfg,
        }
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
    pub fn get_change_with_lamport_lte(
        &self,
        peer: PeerID,
        lamport: Lamport,
    ) -> Option<BlockChangeRef> {
        let ans = self
            .change_store
            .get_change_by_lamport_lte(IdLp::new(peer, lamport))?;
        debug_assert!(ans.lamport <= lamport);
        Some(ans)
    }

    pub fn get_timestamp_of_version(&self, f: &Frontiers) -> Timestamp {
        let mut timestamp = Timestamp::default();
        for id in f.iter() {
            if let Some(change) = self.lookup_change(id) {
                timestamp = timestamp.max(change.timestamp);
            }
        }

        timestamp
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.dag.is_empty() && self.arena.can_import_snapshot()
    }

    /// This is the **only** place to update the `OpLog.changes`
    pub(crate) fn insert_new_change(&mut self, change: Change, from_local: bool) {
        let s = trace_span!(
            "insert_new_change",
            id = ?change.id,
            lamport = change.lamport,
            deps = ?change.deps
        );
        let _enter = s.enter();
        self.dag.handle_new_change(&change, from_local);
        self.history_cache
            .try_lock()
            .unwrap()
            .insert_by_new_change(&change, true, true);
        self.register_container_and_parent_link(&change);
        self.change_store.insert_change(change, true);
    }

    #[inline(always)]
    pub(crate) fn with_history_cache<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut ContainerHistoryCache) -> R,
    {
        let mut history_cache = self.history_cache.try_lock().unwrap();
        f(&mut history_cache)
    }

    pub fn has_history_cache(&self) -> bool {
        self.history_cache.try_lock().unwrap().has_cache()
    }

    pub fn free_history_cache(&self) {
        let mut history_cache = self.history_cache.try_lock().unwrap();
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
        self.insert_new_change(change, true);
        Ok(())
    }

    /// Trim the known part of change
    pub(crate) fn trim_the_known_part_of_change(&self, change: Change) -> Option<Change> {
        let Some(&end) = self.dag.vv().get(&change.id.peer) else {
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

    #[allow(unused)]
    fn check_id_is_not_duplicated(&self, id: ID) -> Result<(), LoroError> {
        let cur_end = self.dag.vv().get(&id.peer).cloned().unwrap_or(0);
        if cur_end > id.counter {
            return Err(LoroError::UsedOpID { id });
        }

        Ok(())
    }

    /// Ensure the new change is greater than the last peer's id and the counter is continuous.
    ///
    /// It can be false when users use detached editing mode and use a custom peer id.
    // This method might be slow and can be optimized if needed in the future.
    pub(crate) fn check_change_greater_than_last_peer_id(
        &self,
        peer: PeerID,
        counter: Counter,
        deps: &Frontiers,
    ) -> Result<(), LoroError> {
        if counter == 0 {
            return Ok(());
        }

        if !self.configure.detached_editing() {
            return Ok(());
        }

        let mut max_last_counter = -1;
        for dep in deps.iter() {
            let dep_vv = self.dag.get_vv(dep).unwrap();
            max_last_counter = max_last_counter.max(dep_vv.get(&peer).cloned().unwrap_or(0) - 1);
        }

        if counter != max_last_counter + 1 {
            return Err(LoroError::ConcurrentOpsWithSamePeerID {
                peer,
                last_counter: max_last_counter,
                current: counter,
            });
        }

        Ok(())
    }

    pub(crate) fn next_id(&self, peer: PeerID) -> ID {
        let cnt = self.dag.vv().get(&peer).copied().unwrap_or(0);
        ID::new(peer, cnt)
    }

    pub(crate) fn vv(&self) -> &VersionVector {
        self.dag.vv()
    }

    pub(crate) fn frontiers(&self) -> &Frontiers {
        self.dag.frontiers()
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
        self.extend_pending_changes_with_unknown_lamport(remote_changes)
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
    pub(crate) fn export_change_store_from(&self, vv: &VersionVector, f: &Frontiers) -> Bytes {
        self.change_store
            .export_from(vv, f, self.vv(), self.frontiers())
    }

    #[inline(always)]
    pub(crate) fn export_change_store_in_range(
        &self,
        vv: &VersionVector,
        f: &Frontiers,
        to_vv: &VersionVector,
        to_frontiers: &Frontiers,
    ) -> Bytes {
        self.change_store.export_from(vv, f, to_vv, to_frontiers)
    }

    #[inline(always)]
    pub(crate) fn export_blocks_from<W: std::io::Write>(&self, vv: &VersionVector, w: &mut W) {
        self.change_store
            .export_blocks_from(vv, self.shallow_since_vv(), self.vv(), w)
    }

    #[inline(always)]
    pub(crate) fn export_blocks_in_range<W: std::io::Write>(&self, spans: &[IdSpan], w: &mut W) {
        self.change_store.export_blocks_in_range(spans, w)
    }

    pub(crate) fn fork_changes_up_to(&self, frontiers: &Frontiers) -> Option<Bytes> {
        let vv = self.dag.frontiers_to_vv(frontiers)?;
        Some(
            self.change_store
                .fork_changes_up_to(self.dag.shallow_since_vv(), frontiers, &vv),
        )
    }

    #[inline(always)]
    pub(crate) fn decode(&mut self, data: ParsedHeaderAndBody) -> Result<ImportStatus, LoroError> {
        decode_oplog(self, data)
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
    #[allow(clippy::type_complexity)]
    pub(crate) fn iter_from_lca_causally(
        &self,
        from: &VersionVector,
        from_frontiers: &Frontiers,
        to: &VersionVector,
        to_frontiers: &Frontiers,
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
        debug!("to_frontiers={:?} vv={:?}", &to_frontiers, to);
        let (common_ancestors, mut diff_mode) =
            self.dag.find_common_ancestor(from_frontiers, to_frontiers);
        if diff_mode == DiffMode::Checkout && to > from {
            diff_mode = DiffMode::Import;
        }

        let common_ancestors_vv = self.dag.frontiers_to_vv(&common_ancestors).unwrap();
        // go from lca to merged_vv
        let diff = common_ancestors_vv.diff(&merged_vv).right;
        let mut iter = self.dag.iter_causal(common_ancestors, diff);
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
                    self.dag.ensure_vv_for(&inner.data);
                    inner_vv.extend_to_include_vv(inner.data.vv.get().unwrap().iter());
                    let peer = inner.data.peer;
                    let cnt = inner
                        .data
                        .cnt
                        .max(cur_cnt)
                        .max(common_ancestors_vv.get(&peer).copied().unwrap_or(0));
                    let dag_node_end = (inner.data.cnt + inner.data.len as Counter)
                        .min(merged_vv.get(&peer).copied().unwrap_or(0));
                    trace!("vv: {:?}", self.dag.vv());
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
        let total_dag_node = self.dag.total_parsed_dag_node();
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
            (get_sys_timestamp() as Timestamp + 500) / 1000
        } else {
            0
        }
    }

    #[inline(never)]
    pub(crate) fn idlp_to_id(&self, id: loro_common::IdLp) -> Option<ID> {
        let change = self.change_store.get_change_by_lamport_lte(id)?;

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

    /// NOTE: This may return a op that includes the given id, not necessarily start with the given id
    pub(crate) fn get_op_that_includes(&self, id: ID) -> Option<BlockOpRef> {
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
        self.change_store
            .flush_and_compact(self.dag.vv(), self.dag.frontiers());
    }

    #[inline]
    pub fn change_store_kv_size(&self) -> usize {
        self.change_store.kv_size()
    }

    pub fn encode_change_store(&self) -> bytes::Bytes {
        self.change_store
            .encode_all(self.dag.vv(), self.dag.frontiers())
    }

    pub fn check_dag_correctness(&self) {
        self.dag.check_dag_correctness();
    }

    pub fn shallow_since_vv(&self) -> &ImVersionVector {
        self.dag.shallow_since_vv()
    }

    pub fn shallow_since_frontiers(&self) -> &Frontiers {
        self.dag.shallow_since_frontiers()
    }

    pub fn is_shallow(&self) -> bool {
        !self.dag.shallow_since_vv().is_empty()
    }

    pub fn get_greatest_timestamp(&self, frontiers: &Frontiers) -> Timestamp {
        let mut max_timestamp = Timestamp::default();
        for id in frontiers.iter() {
            let change = self.get_change_at(id).unwrap();
            if change.timestamp > max_timestamp {
                max_timestamp = change.timestamp;
            }
        }

        max_timestamp
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
        commit_msg: change.commit_msg.clone(),
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
                    value: (**value).clone(),
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
