use std::{cell::RefCell, collections::VecDeque, sync::Arc};

use crate::sync::{AtomicU64, Mutex};
use either::Either;
use loro_common::{
    ContainerID, Counter, CounterSpan, HasIdSpan, IdSpan, LoroError, LoroResult, LoroValue, PeerID,
    ID,
};
use parking_lot::lock_api::ReentrantMutex;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use tracing::{debug_span, info_span, instrument};

use crate::{
    change::{get_sys_timestamp, Timestamp},
    cursor::{AbsolutePosition, Cursor},
    delta::TreeExternalDiff,
    event::{container_id_estimated_bytes, Diff, EventTriggerKind, KeptChange},
    version::Frontiers,
    ContainerDiff, DiffEvent, DocDiff, LoroDoc, Subscription,
};

#[cfg(test)]
mod atomicity_tests;
pub(crate) mod movable_list;
#[cfg(test)]
mod split_span_tests;

/// A batch of diffs.
///
/// You can use `loroDoc.apply_diff(diff)` to apply the diff to the document.
#[derive(Debug, Clone, Default)]
pub struct DiffBatch {
    pub cid_to_events: FxHashMap<ContainerID, Diff>,
    pub order: Vec<ContainerID>,
    /// Set on batches from [`LoroDoc::diff`]: a mergeable child the batch re-activates carries
    /// its full state (no entry means empty), which `apply_diff` aligns with whatever hidden
    /// state the target doc keeps at that deterministic cid. Unset batches (local events,
    /// hand-built ones) are applied incrementally, as a doc that shares the source's hidden
    /// state needs. Local events must stay unset: a plain re-ensure has no child entry because
    /// the child is unchanged, which a full-state batch would read as "empty". Import and
    /// checkout events do carry full states for revived children, so they may be set. See
    /// context/mergeable-containers.md.
    pub full_state: bool,
}

impl DiffBatch {
    pub fn new(diff: Vec<DocDiff>) -> Self {
        let mut map: FxHashMap<ContainerID, Diff> = Default::default();
        let mut order: Vec<ContainerID> = Vec::with_capacity(diff.len());
        for d in diff.into_iter() {
            for item in d.diff.into_iter() {
                let old = map.insert(item.id.clone(), item.diff);
                assert!(old.is_none(), "Duplicate container ID in diff events");
                order.push(item.id.clone());
            }
        }

        Self {
            cid_to_events: map,
            order,
            full_state: false,
        }
    }

    /// Like [`DiffBatch::new`], but a container that kept its state while its event reports a
    /// full-state revival contributes its actual change. Use it for a batch applied to the doc
    /// it was computed on (revert, undo). See [`crate::event::KeptChange`].
    pub(crate) fn from_changes(diff: Vec<DocDiff>) -> Self {
        let mut map: FxHashMap<ContainerID, Diff> = Default::default();
        let mut order: Vec<ContainerID> = Vec::with_capacity(diff.len());
        for d in diff.into_iter() {
            for item in d.diff.into_iter() {
                let change = match item.kept {
                    None => item.diff,
                    Some(KeptChange::Unchanged) => continue,
                    Some(KeptChange::Changed(change)) => change,
                };
                let old = map.insert(item.id.clone(), change);
                assert!(old.is_none(), "Duplicate container ID in diff events");
                order.push(item.id);
            }
        }

        Self {
            cid_to_events: map,
            order,
            full_state: false,
        }
    }

    /// Composes `other` after `self`.
    ///
    /// [`DiffBatch::full_state`] composes as follows. An empty batch is the identity, so
    /// composing into one adopts `other`'s flag. Two full-state batches stay full-state: a
    /// mergeable child that `other` re-activates was hidden at the version where the batches
    /// meet, so `self` has no entry for it, and a child `self` re-activated followed by
    /// increments is still a full state. Two incremental batches stay incremental. A
    /// full-state batch and an incremental one cannot be composed: re-activated children would
    /// mix full states and increments on hidden state, which neither mode can apply.
    ///
    /// # Panics
    ///
    /// If both batches are non-empty and exactly one of them is full-state.
    pub fn compose(&mut self, other: &Self) {
        if other.cid_to_events.is_empty() {
            return;
        }
        if self.cid_to_events.is_empty() {
            self.full_state = other.full_state;
        }
        assert_eq!(
            self.full_state, other.full_state,
            "cannot compose a full-state DiffBatch with an incremental one"
        );

        for (id, diff) in other.iter() {
            if let Some(this_diff) = self.cid_to_events.get_mut(id) {
                this_diff.compose_ref(diff);
            } else {
                self.cid_to_events.insert(id.clone(), diff.clone());
                self.order.push(id.clone());
            }
        }
    }

    /// Inverses run in sequence, so a later inverse map write must win even
    /// when it restores a value with an older operation timestamp.
    pub(crate) fn compose_inverse(&mut self, other: Self) {
        if other.cid_to_events.is_empty() {
            return;
        }
        if self.cid_to_events.is_empty() {
            *self = other;
            return;
        }
        assert_eq!(
            self.full_state, other.full_state,
            "cannot compose a full-state DiffBatch with an incremental one"
        );

        for (id, diff) in other.into_iter() {
            self.compose_inverse_one(id, diff);
        }
    }

    /// [`Self::compose_inverse`] with a batch of one container's diff.
    pub(crate) fn compose_inverse_one(&mut self, id: ContainerID, diff: Diff) {
        if let Some(this_diff) = self.cid_to_events.get_mut(&id) {
            match (this_diff, diff) {
                (Diff::Map(a), Diff::Map(b)) => {
                    a.updated.extend(b.updated);
                }
                (a, b) => a.compose_ref(&b),
            }
        } else {
            self.cid_to_events.insert(id.clone(), diff);
            self.order.push(id);
        }
    }

    pub fn transform(&mut self, other: &Self, left_priority: bool) {
        if other.cid_to_events.is_empty() || self.cid_to_events.is_empty() {
            return;
        }

        for (idx, diff) in self.cid_to_events.iter_mut() {
            if let Some(b_diff) = other.cid_to_events.get(idx) {
                diff.transform(b_diff, left_priority);
            }
        }
    }

    pub fn clear(&mut self) {
        self.cid_to_events.clear();
        self.order.clear();
        self.full_state = false;
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ContainerID, &Diff)> + '_ {
        self.order
            .iter()
            .map(|cid| (cid, self.cid_to_events.get(cid).unwrap()))
    }

    #[allow(clippy::should_implement_trait)]
    pub fn into_iter(self) -> impl Iterator<Item = (ContainerID, Diff)> {
        let mut cid_to_events = self.cid_to_events;
        self.order.into_iter().map(move |cid| {
            let d = cid_to_events.remove(&cid).unwrap();
            (cid, d)
        })
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        let order_bytes = self.order.capacity() * std::mem::size_of::<ContainerID>();
        let map_table_bytes = self.cid_to_events.capacity()
            * (std::mem::size_of::<ContainerID>() + std::mem::size_of::<Diff>() + 8);
        let entry_bytes: usize = self
            .cid_to_events
            .iter()
            .map(|(cid, diff)| {
                container_id_estimated_bytes(cid).saturating_add(diff.estimated_bytes())
            })
            .sum();
        order_bytes
            .saturating_add(map_table_bytes)
            .saturating_add(entry_bytes)
    }
}

fn transform_cursor(
    cursor_with_pos: &mut CursorWithPos,
    remote_diff: &DiffBatch,
    doc: &LoroDoc,
    container_remap: &FxHashMap<ContainerID, ContainerID>,
) {
    let mut container_changed = false;
    let mut cid = &cursor_with_pos.cursor.container;
    while let Some(new_cid) = container_remap.get(cid) {
        cid = new_cid;
        container_changed = true;
    }

    if cursor_with_pos.cursor.id.is_none() {
        // We don't need to transform a cursor that always points to the leftmost or rightmost position
        if container_changed {
            cursor_with_pos.cursor.container = cid.clone();
        }
        return;
    }

    if let Some(diff) = remote_diff.cid_to_events.get(cid) {
        let new_pos = diff.transform_cursor(cursor_with_pos.pos.pos, false);
        cursor_with_pos.pos.pos = new_pos;
    };

    let new_pos = cursor_with_pos.pos.pos;
    // Cursors come from the user's `on_push` and may name a missing container
    let Some(handler) = doc.get_handler(cid.clone()) else {
        return;
    };
    match handler {
        crate::handler::Handler::Text(h) => {
            let Some(new_cursor) = h.get_cursor_internal(new_pos, cursor_with_pos.pos.side, false)
            else {
                return;
            };

            cursor_with_pos.cursor = new_cursor;
        }
        crate::handler::Handler::List(h) => {
            let Some(new_cursor) = h.get_cursor(new_pos, cursor_with_pos.pos.side) else {
                return;
            };

            cursor_with_pos.cursor = new_cursor;
        }
        crate::handler::Handler::MovableList(h) => {
            let Some(new_cursor) = h.get_cursor(new_pos, cursor_with_pos.pos.side) else {
                return;
            };

            cursor_with_pos.cursor = new_cursor;
        }
        crate::handler::Handler::Map(_) => {}
        crate::handler::Handler::Tree(_) => {}
        crate::handler::Handler::Unknown(_) => {}
        #[cfg(feature = "counter")]
        crate::handler::Handler::Counter(_) => {}
    }
}

/// UndoManager is responsible for managing undo/redo from the current peer's perspective.
///
/// Undo/local is local: it cannot be used to undone the changes made by other peers.
/// If you want to undo changes made by other peers, you may need to use the time travel feature.
///
/// PeerID cannot be changed during the lifetime of the UndoManager
pub struct UndoManager {
    peer: Arc<AtomicU64>,
    container_remap: Arc<Mutex<FxHashMap<ContainerID, ContainerID>>>,
    element_remap: Mutex<movable_list::ElementRemap>,
    inner: Arc<parking_lot::ReentrantMutex<RefCell<UndoManagerInner>>>,
    _peer_id_change_sub: Subscription,
    _undo_sub: Subscription,
    doc: LoroDoc,
    /// Dropped last, after every reference to `inner`: the document's undo bytes
    /// shrink here.
    _stats_mark: StatsMarkOnDrop,
}

impl std::fmt::Debug for UndoManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UndoManager")
            .field("peer", &self.peer)
            .field("container_remap", &self.container_remap)
            .field("inner", &self.inner)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoOrRedo {
    Undo,
    Redo,
}

impl UndoOrRedo {
    fn opposite(&self) -> UndoOrRedo {
        match self {
            Self::Undo => Self::Redo,
            Self::Redo => Self::Undo,
        }
    }
}

/// When a undo/redo item is pushed, the undo manager will call the on_push callback to get the meta data of the undo item.
/// The returned cursors will be recorded for a new pushed undo item.
pub type OnPush = Box<
    dyn for<'a> Fn(UndoOrRedo, CounterSpan, Option<DiffEvent<'a>>) -> UndoItemMeta + Send + Sync,
>;
pub type OnPop = Box<dyn Fn(UndoOrRedo, CounterSpan, UndoItemMeta) + Send + Sync>;

pub(crate) struct UndoManagerInner {
    next_counter: Option<Counter>,
    undo_stack: Stack,
    redo_stack: Stack,
    processing_undo: bool,
    /// Inverses of the commits made while `processing_undo`: the undo/redo's
    /// own ops, whose inverse is the opposite stack's item. `None` once one
    /// was missing.
    processing_inverses: Option<Vec<Arc<LocalInverse>>>,
    paused: bool,
    last_undo_time: i64,
    merge_interval_in_ms: i64,
    max_stack_size: usize,
    max_undo_bytes: usize,
    exclude_origin_prefixes: Vec<Box<str>>,
    last_popped_selection: Option<Vec<CursorWithPos>>,
    on_push: Option<OnPush>,
    on_pop: Option<OnPop>,
    group: Option<UndoGroup>,
}

impl std::fmt::Debug for UndoManagerInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UndoManagerInner")
            .field("latest_counter", &self.next_counter)
            .field("undo_stack", &self.undo_stack)
            .field("redo_stack", &self.redo_stack)
            .field("processing_undo", &self.processing_undo)
            .field("last_undo_time", &self.last_undo_time)
            .field("merge_interval", &self.merge_interval_in_ms)
            .field("max_stack_size", &self.max_stack_size)
            .field("exclude_origin_prefixes", &self.exclude_origin_prefixes)
            .field("group", &self.group)
            .finish()
    }
}

#[derive(Debug, Clone, Default)]
struct UndoGroup {
    start_counter: Counter,
    affected_cids: FxHashSet<ContainerID>,
}

impl UndoGroup {
    pub fn new(start_counter: Counter) -> Self {
        Self {
            start_counter,
            affected_cids: Default::default(),
        }
    }
}

/// The inverse of one local commit, recorded while its ops were applied
/// (`txn::LocalInverseRecorder`). Applying `diff` right after `span` gives the
/// state at the commit's deps. See context/undo-log.md.
#[derive(Debug)]
pub(crate) struct CommitInverse {
    pub(crate) span: IdSpan,
    pub(crate) diff: DiffBatch,
    /// The value each map key written by `span` has right after it. A checkout
    /// reports no change for a key whose value ends where it started (see
    /// [`prune_unchanged_map_entries`]).
    pub(crate) map_values_after: FxHashMap<(ContainerID, crate::InternalString), Option<LoroValue>>,
    /// The identity records of the span's movable-list ops (one per list).
    pub(crate) movable: Vec<movable_list::RecordedInverse>,
    /// Each counter increment of the span, in op order (see
    /// [`counter_inverses`]).
    pub(crate) counter_ops: Vec<(ContainerID, f64)>,
}

/// The counter part of the inverse of `commits` (oldest first) over `span`:
/// as the checkout's counter diff calculator computes it, starting at 0 and
/// subtracting each increment in op order (float addition is not associative,
/// so composing per-op inverses could give another value). Counters created in
/// `span` are left out, as in [`drop_containers_created_in`].
fn counter_inverses<'a>(
    commits: impl Iterator<Item = &'a CommitInverse>,
    span: IdSpan,
    diff: &mut DiffBatch,
) {
    #[cfg(feature = "counter")]
    {
        let mut sums: Vec<(&ContainerID, f64)> = Vec::new();
        for (cid, increment) in commits.flat_map(|c| c.counter_ops.iter()) {
            if created_in(cid, span) {
                continue;
            }
            match sums.iter_mut().find(|(c, _)| *c == cid) {
                Some((_, sum)) => *sum -= increment,
                None => sums.push((cid, 0. - increment)),
            }
        }
        for (cid, sum) in sums {
            diff.order.push(cid.clone());
            diff.cid_to_events.insert(cid.clone(), Diff::Counter(sum));
        }
    }
    #[cfg(not(feature = "counter"))]
    let _ = (commits, span, diff);
}

impl CommitInverse {
    /// Applies the rules that make a composed inverse equal what a checkout
    /// from the end of `span` to its deps reports.
    pub(crate) fn finish(&mut self, normalize: bool) {
        for record in &mut self.movable {
            record.mark_created_in(self.span);
        }
        prune_unchanged_map_entries(&mut self.diff, &self.map_values_after);
        drop_containers_created_in(&mut self.diff, self.span);
        if normalize {
            normalize_deltas(&mut self.diff);
        }
    }

    fn estimated_bytes(&self) -> usize {
        let map_bytes: usize = self
            .map_values_after
            .iter()
            .map(|((cid, key), v)| {
                container_id_estimated_bytes(cid)
                    + key.len()
                    + std::mem::size_of::<Option<LoroValue>>()
                    + v.as_ref().map_or(0, |v| {
                        v.estimated_heap_bytes()
                            .saturating_sub(std::mem::size_of::<LoroValue>())
                    })
            })
            .sum();
        self.diff.estimated_bytes()
            + map_bytes
            + self.counter_ops.len() * std::mem::size_of::<(ContainerID, f64)>()
            + self
                .movable
                .iter()
                .map(|r| r.estimated_bytes())
                .sum::<usize>()
    }
}

/// The inverses of a run of local commits, each continuing right after the
/// previous one with no other op in between. Merging an undo item only appends
/// (the commits are shared); [`Self::seal`] composes them when the item is
/// undone.
#[derive(Debug, Clone)]
pub(crate) struct LocalInverse {
    pub(crate) span: IdSpan,
    /// The deps of the first commit.
    pub(crate) deps: Frontiers,
    /// Oldest first.
    commits: Vec<Arc<CommitInverse>>,
}

/// A [`LocalInverse`] composed into one inverse of its whole span: what undo
/// used to get by checking the document out at the span's last op and then at
/// its deps.
#[derive(Debug)]
pub(crate) struct SealedInverse {
    pub(crate) span: IdSpan,
    pub(crate) deps: Frontiers,
    pub(crate) diff: DiffBatch,
    pub(crate) movable: Vec<movable_list::RecordedInverse>,
}

impl LocalInverse {
    pub(crate) fn new(commit: CommitInverse, deps: Frontiers) -> Self {
        Self {
            span: commit.span,
            deps,
            commits: vec![Arc::new(commit)],
        }
    }

    /// `a` followed by `b`, if both are recorded and `b` continues right after
    /// `a` with no other op in between.
    pub(crate) fn chain(
        a: Option<Arc<LocalInverse>>,
        b: Option<&Arc<LocalInverse>>,
    ) -> Option<Arc<LocalInverse>> {
        let (mut a, b) = (a?, b?);
        let continues = b.span.peer == a.span.peer
            && b.span.counter.start == a.span.counter.end
            && b.deps == Frontiers::from_id(a.span.id_last());
        if !continues {
            return None;
        }
        let this = Arc::make_mut(&mut a);
        this.span.counter.end = b.span.counter.end;
        this.commits.extend(b.commits.iter().cloned());
        Some(a)
    }

    /// The inverse of the whole span. `None` if it cannot be composed (a
    /// movable list edited by two commits; undo checks out instead).
    pub(crate) fn seal(&self) -> Option<SealedInverse> {
        if let [only] = self.commits.as_slice() {
            let mut diff = only.diff.clone();
            counter_inverses(self.commits.iter().map(|c| &**c), self.span, &mut diff);
            return Some(SealedInverse {
                span: self.span,
                deps: self.deps.clone(),
                diff,
                movable: only.movable.clone(),
            });
        }
        let mut sealed = CommitInverse {
            span: self.span,
            diff: DiffBatch::default(),
            map_values_after: FxHashMap::default(),
            movable: Vec::new(),
            counter_ops: Vec::new(),
        };
        // Each list's records cover one commit; a list edited by two cannot
        // combine them.
        for commit in &self.commits {
            for r in &commit.movable {
                if sealed.movable.iter().any(|o| o.cid == r.cid) {
                    return None;
                }
                sealed.movable.push(r.clone());
            }
            sealed.map_values_after.extend(
                commit
                    .map_values_after
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone())),
            );
        }
        // Newest first: undoing the run undoes its last commit first.
        for commit in self.commits.iter().rev() {
            sealed.diff.compose_inverse(commit.diff.clone());
        }
        sealed.finish(true);
        counter_inverses(
            self.commits.iter().map(|c| &**c),
            self.span,
            &mut sealed.diff,
        );
        Some(SealedInverse {
            span: self.span,
            deps: self.deps.clone(),
            diff: sealed.diff,
            movable: sealed.movable,
        })
    }

    fn estimated_bytes(&self) -> usize {
        self.commits.iter().map(|c| c.estimated_bytes()).sum()
    }
}

impl SealedInverse {
    /// The full event-level inverse: the recorded diffs and the movable lists'
    /// positional diffs, built now ([`movable_list::RecordedInverse::positional_diff`]).
    pub(crate) fn full_diff(&self, doc: &LoroDoc) -> DiffBatch {
        let mut diff = self.diff.clone();
        for record in &self.movable {
            if let Some(d) = record.positional_diff(doc) {
                diff.order.push(record.cid.clone());
                diff.cid_to_events.insert(record.cid.clone(), d);
            }
        }
        diff
    }
}

/// Drops map entries that restore the value a key already has after the span:
/// a checkout reports a key only when its value changes. Entries for keys that
/// did not exist before (unset) always stay, as in a checkout.
pub(crate) fn prune_unchanged_map_entries(
    diff: &mut DiffBatch,
    values_after: &FxHashMap<(ContainerID, crate::InternalString), Option<LoroValue>>,
) {
    for (cid, d) in diff.cid_to_events.iter_mut() {
        let Diff::Map(m) = d else {
            continue;
        };
        m.updated.retain(|key, restored| {
            if restored.idlp == crate::delta::ResolvedMapValue::new_unset().idlp {
                return true;
            }
            let Some(after) = values_after.get(&(cid.clone(), key.clone())) else {
                return true;
            };
            let restored = restored.value.as_ref().map(|v| v.to_value());
            restored != *after
        });
    }
}

/// Merges adjacent items of the same kind, as the deltas a checkout reports
/// are built. Composing per-op inverses leaves them split (`[delete 1, delete
/// 1]` instead of `[delete 2]`).
pub(crate) fn normalize_deltas(diff: &mut DiffBatch) {
    fn rebuild<V, A>(d: &loro_delta::DeltaRope<V, A>) -> loro_delta::DeltaRope<V, A>
    where
        V: loro_delta::delta_trait::DeltaValue + Clone,
        A: loro_delta::delta_trait::DeltaAttr + Clone,
    {
        let mut out = loro_delta::DeltaRope::new();
        for item in d.iter() {
            out.push(item.clone());
        }
        out
    }
    /// Also joins adjacent text inserts with the same attributes, which come
    /// from different chunks and do not merge by `push`.
    fn rebuild_text(d: &crate::event::TextDiff) -> crate::event::TextDiff {
        let mut items: Vec<crate::event::TextDiffItem> = Vec::new();
        for item in d.iter() {
            if let (
                Some(loro_delta::DeltaItem::Replace {
                    value: last_value,
                    attr: last_attr,
                    delete: last_delete,
                }),
                loro_delta::DeltaItem::Replace {
                    value,
                    attr,
                    delete: 0,
                },
            ) = (items.last_mut(), item)
            {
                if last_attr == attr && *last_delete == 0 {
                    last_value.extend(value.as_str());
                    continue;
                }
            }
            items.push(item.clone());
        }
        let mut out = loro_delta::DeltaRope::new();
        for item in items {
            out.push(item);
        }
        out
    }
    for d in diff.cid_to_events.values_mut() {
        match d {
            Diff::Text(t) => *t = rebuild_text(t),
            Diff::List(l) => *l = rebuild(l),
            _ => {}
        }
    }
}

/// Whether `span`'s ops created `cid`.
pub(crate) fn created_in(cid: &ContainerID, span: IdSpan) -> bool {
    matches!(cid, ContainerID::Normal { peer, counter, .. }
        if span.contains(ID::new(*peer, *counter)))
}

/// Containers created by `span`'s ops do not exist at its deps, so a checkout
/// there reports no diff for them; the inverse must not either.
pub(crate) fn drop_containers_created_in(diff: &mut DiffBatch, span: IdSpan) {
    let created = |cid: &ContainerID| created_in(cid, span);
    if diff.cid_to_events.keys().any(created) {
        diff.cid_to_events.retain(|cid, _| !created(cid));
        diff.order.retain(|cid| !created(cid));
    }
}

type SharedUndoInner = parking_lot::ReentrantMutex<RefCell<UndoManagerInner>>;

/// The undo manager's lock, marking the document's memory statistics stale when
/// released (still under the lock). Undo stacks change outside the document's own
/// locks, e.g. in event callbacks; see `crate::loro::DocStatsDirty`.
struct MarkedUndoGuard<'a> {
    guard: parking_lot::ReentrantMutexGuard<'a, RefCell<UndoManagerInner>>,
    dirty: &'a crate::loro::DocStatsDirty,
}

impl<'a> MarkedUndoGuard<'a> {
    fn lock(inner: &'a SharedUndoInner, dirty: &'a crate::loro::DocStatsDirty) -> Self {
        Self {
            guard: inner.lock(),
            dirty,
        }
    }
}

impl std::ops::Deref for MarkedUndoGuard<'_> {
    type Target = RefCell<UndoManagerInner>;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl Drop for MarkedUndoGuard<'_> {
    fn drop(&mut self) {
        self.dirty.mark();
    }
}

struct StatsMarkOnDrop(Arc<crate::loro::DocStatsDirty>);

impl Drop for StatsMarkOnDrop {
    fn drop(&mut self) {
        self.0.mark();
    }
}

struct ProcessingUndoGuard {
    inner: Arc<parking_lot::ReentrantMutex<RefCell<UndoManagerInner>>>,
}

impl Drop for ProcessingUndoGuard {
    fn drop(&mut self) {
        let lock = self.inner.lock();
        let mut inner = lock.borrow_mut();
        inner.processing_undo = false;
        inner.processing_inverses = None;
    }
}

#[derive(Debug)]
struct Stack {
    stack: VecDeque<(VecDeque<StackItem>, Arc<Mutex<DiffBatch>>)>,
    size: usize,
}

#[derive(Debug, Clone)]
struct StackItem {
    span: CounterSpan,
    meta: UndoItemMeta,
    affected_cids: FxHashSet<ContainerID>,
    /// The inverse of the item's ops, when every op of it was recorded and
    /// they form one run with no other op in between. Undo then uses it
    /// instead of checking the document out.
    inverse: Option<Arc<LocalInverse>>,
}

impl StackItem {
    fn estimated_bytes(&self) -> usize {
        let value_bytes = self
            .meta
            .value
            .estimated_heap_bytes()
            .saturating_sub(std::mem::size_of::<LoroValue>());
        let cursors_bytes = self.meta.cursors.capacity() * std::mem::size_of::<CursorWithPos>()
            + self
                .meta
                .cursors
                .iter()
                .map(|c| container_id_estimated_bytes(&c.cursor.container))
                .sum::<usize>();
        let cids_bytes = self.affected_cids.capacity() * (std::mem::size_of::<ContainerID>() + 8)
            + self
                .affected_cids
                .iter()
                .map(container_id_estimated_bytes)
                .sum::<usize>();
        let inverse_bytes = self.inverse.as_ref().map_or(0, |inv| inv.estimated_bytes());
        std::mem::size_of::<Self>()
            .saturating_add(value_bytes)
            .saturating_add(cursors_bytes)
            .saturating_add(cids_bytes)
            .saturating_add(inverse_bytes)
    }
}

/// The metadata of an undo item.
///
/// The cursors inside the metadata will be transformed by remote operations as well.
/// So that when the item is popped, users can restore the cursors position correctly.
#[derive(Debug, Default, Clone)]
pub struct UndoItemMeta {
    pub value: LoroValue,
    pub cursors: Vec<CursorWithPos>,
}

#[derive(Debug, Clone)]
pub struct CursorWithPos {
    pub cursor: Cursor,
    pub pos: AbsolutePosition,
}

impl UndoItemMeta {
    pub fn new() -> Self {
        Self {
            value: LoroValue::Null,
            cursors: Default::default(),
        }
    }

    /// It's assumed that the cursor is just acquired before the ops that
    /// need to be undo/redo.
    ///
    /// We need to rely on the validity of the original pos value
    pub fn add_cursor(&mut self, cursor: &Cursor) {
        self.cursors.push(CursorWithPos {
            cursor: cursor.clone(),
            pos: AbsolutePosition {
                pos: cursor.origin_pos,
                side: cursor.side,
            },
        });
    }

    pub fn set_value(&mut self, value: LoroValue) {
        self.value = value;
    }
}

impl Stack {
    pub fn new() -> Self {
        let mut stack = VecDeque::new();
        stack.push_back((VecDeque::new(), Arc::new(Mutex::new(Default::default()))));
        Stack { stack, size: 0 }
    }

    /// Peek the top-most StackItem's metadata without modifying the stack.
    ///
    /// Returns None if the stack is empty.
    fn peek_top_meta(&self) -> Option<UndoItemMeta> {
        if self.is_empty() {
            return None;
        }

        for (items, _) in self.stack.iter().rev() {
            if let Some(item) = items.back() {
                return Some(item.meta.clone());
            }
        }

        None
    }

    /// Peek the top-most StackItem's counter span without modifying the stack.
    ///
    /// Returns None if the stack is empty.
    fn peek_top_span(&self) -> Option<CounterSpan> {
        self.stack
            .iter()
            .rev()
            .find_map(|(items, _)| items.back().map(|item| item.span))
    }

    pub fn pop(&mut self) -> Option<(StackItem, Arc<Mutex<DiffBatch>>)> {
        while self.stack.back().unwrap().0.is_empty() && self.stack.len() > 1 {
            let (_, diff) = self.stack.pop_back().unwrap();
            let diff = diff.lock();
            if !diff.cid_to_events.is_empty() {
                self.stack.back_mut().unwrap().1.lock().compose(&diff);
            }
        }

        if self.stack.len() == 1 && self.stack.back().unwrap().0.is_empty() {
            // If the stack is empty, we need to clear the remote diff
            self.stack.back_mut().unwrap().1.lock().clear();
            return None;
        }

        self.size -= 1;
        let last = self.stack.back_mut().unwrap();
        last.0.pop_back().map(|x| (x, last.1.clone()))
        // If this row in stack is empty, we don't pop it right away
        // Because we still need the remote diff to be available.
        // Cursor position transformation relies on the remote diff in the same row.
    }

    #[allow(unused)]
    pub fn push(&mut self, span: CounterSpan, meta: UndoItemMeta) {
        self.push_with_cids(span, meta, Default::default())
    }

    pub fn push_with_cids(
        &mut self,
        span: CounterSpan,
        meta: UndoItemMeta,
        affected_cids: FxHashSet<ContainerID>,
    ) {
        self.push_with_merge_cids(span, meta, affected_cids, None, false, None)
    }

    fn push_with_cids_and_inverse(
        &mut self,
        span: CounterSpan,
        meta: UndoItemMeta,
        affected_cids: FxHashSet<ContainerID>,
        inverse: Option<Arc<LocalInverse>>,
    ) {
        self.push_with_merge_cids(span, meta, affected_cids, inverse, false, None)
    }

    fn restore_popped(&mut self, item: StackItem, remote_diff: &Arc<Mutex<DiffBatch>>) {
        let last = self.stack.back_mut().unwrap();
        assert!(
            Arc::ptr_eq(&last.1, remote_diff),
            "failed undo lost its remote-diff row"
        );
        last.0.push_back(item);
        self.size += 1;
    }

    #[allow(unused)]
    pub fn push_with_merge(
        &mut self,
        span: CounterSpan,
        meta: UndoItemMeta,
        can_merge: bool,
        group: Option<&UndoGroup>,
    ) {
        self.push_with_merge_cids(span, meta, Default::default(), None, can_merge, group);
    }

    pub fn push_with_merge_cids(
        &mut self,
        span: CounterSpan,
        meta: UndoItemMeta,
        affected_cids: FxHashSet<ContainerID>,
        inverse: Option<Arc<LocalInverse>>,
        can_merge: bool,
        group: Option<&UndoGroup>,
    ) {
        let last = self.stack.back_mut().unwrap();
        let last_remote_diff = last.1.lock();

        // Check if the remote diff is disjoint with the current undo group
        let is_disjoint_group = group.is_some_and(|g| {
            g.affected_cids.iter().all(|cid| {
                last_remote_diff
                    .cid_to_events
                    .get(cid)
                    .is_none_or(|diff| diff.is_empty())
            })
        });

        // Can't merge if remote diffs exist and it's not disjoint with the current undo group
        let should_create_new_entry =
            !last_remote_diff.cid_to_events.is_empty() && !is_disjoint_group;

        if should_create_new_entry {
            // Create a new entry in the stack
            drop(last_remote_diff);
            let mut v = VecDeque::new();
            v.push_back(StackItem {
                span,
                meta,
                affected_cids,
                inverse,
            });
            self.stack
                .push_back((v, Arc::new(Mutex::new(DiffBatch::default()))));
            self.size += 1;
            return;
        }

        // Try to merge with the previous entry if allowed
        if can_merge {
            if let Some(last_span) = last.0.back_mut() {
                if last_span.span.end == span.start {
                    // Merge spans by extending the end of the last span
                    last_span.span.end = span.end;
                    last_span.inverse =
                        LocalInverse::chain(last_span.inverse.take(), inverse.as_ref());
                    // An empty set means the checkpoint's containers are unknown.
                    // Merging a known span cannot make that older span known.
                    if last_span.affected_cids.is_empty() || affected_cids.is_empty() {
                        last_span.affected_cids.clear();
                    } else {
                        last_span.affected_cids.extend(affected_cids);
                    }
                    return;
                }
            }
        }

        // Add as a new item to the existing entry
        self.size += 1;
        last.0.push_back(StackItem {
            span,
            meta,
            affected_cids,
            inverse,
        });
    }

    pub fn compose_remote_event(&mut self, diff: &[&ContainerDiff]) {
        if self.is_empty() {
            return;
        }

        let remote_diff = &mut self.stack.back_mut().unwrap().1;
        let mut remote_diff = remote_diff.lock();
        for e in diff {
            // Transform against what actually changed. A re-activated mergeable child's event
            // is its full state, but its content (and the undo items' positions in it) stayed.
            let Some(change) = e.change() else {
                continue;
            };
            if let Some(d) = remote_diff.cid_to_events.get_mut(&e.id) {
                d.compose_ref(change);
            } else {
                remote_diff
                    .cid_to_events
                    .insert(e.id.clone(), change.clone());
                remote_diff.order.push(e.id.clone());
            }
        }
    }

    pub fn transform_based_on_this_delta(&mut self, diff: &DiffBatch) {
        if self.stack.is_empty() {
            return;
        }
        let remote_diff = &mut self.stack.back_mut().unwrap().1;
        remote_diff.lock().transform(diff, false);
    }

    pub fn clear(&mut self) {
        self.stack = VecDeque::new();
        self.stack.push_back((VecDeque::new(), Default::default()));
        self.size = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    pub fn len(&self) -> usize {
        self.size
    }

    fn ensure_trailing_empty_row(&mut self) {
        if self.stack.is_empty() {
            self.stack
                .push_back((VecDeque::new(), Arc::new(Mutex::new(Default::default()))));
        }
    }

    fn pop_front(&mut self) {
        self.pop_front_inner(false);
    }

    /// Returns the change to this stack's byte estimate without walking retained items.
    fn pop_front_inner(&mut self, account_bytes: bool) -> usize {
        if self.is_empty() {
            return 0;
        }
        let mut freed = 0usize;
        while self
            .stack
            .front()
            .is_some_and(|(items, _)| items.is_empty())
        {
            let (items, diff) = self.stack.pop_front().unwrap();
            if account_bytes {
                freed = freed
                    .saturating_add(items.capacity() * std::mem::size_of::<StackItem>())
                    .saturating_add(diff.lock().estimated_bytes());
            }
        }
        self.size -= 1;
        let first = self.stack.front_mut().unwrap();
        let item = first.0.pop_front().expect("nonempty undo row");
        if account_bytes {
            freed = freed.saturating_add(item.estimated_bytes());
        }
        if first.0.is_empty() {
            let (items, diff) = self.stack.pop_front().unwrap();
            if account_bytes {
                freed = freed
                    .saturating_add(items.capacity() * std::mem::size_of::<StackItem>())
                    .saturating_add(diff.lock().estimated_bytes());
            }
        }
        // Empty rows contain no nested allocations; the outer deque keeps its capacity.
        self.ensure_trailing_empty_row();
        freed
    }

    fn set_top_meta(&mut self, meta: UndoItemMeta) {
        let Some(top) = self.stack.back_mut() else {
            return;
        };
        let Some(last) = top.0.back_mut() else {
            return;
        };
        last.meta = meta;
    }

    fn estimated_bytes(&self) -> usize {
        let rows_vec_bytes = self.stack.capacity()
            * std::mem::size_of::<(VecDeque<StackItem>, Arc<Mutex<DiffBatch>>)>();
        let content_bytes: usize = self
            .stack
            .iter()
            .map(|(items, remote_diff)| {
                let items_bytes = items.capacity() * std::mem::size_of::<StackItem>()
                    + items.iter().map(StackItem::estimated_bytes).sum::<usize>();
                items_bytes.saturating_add(remote_diff.lock().estimated_bytes())
            })
            .sum();
        rows_vec_bytes.saturating_add(content_bytes)
    }

    fn has_unknown_affected_cids(&self) -> bool {
        self.stack
            .iter()
            .any(|(items, _)| items.iter().any(|item| item.affected_cids.is_empty()))
    }

    fn collect_relevant_cids(&self, out: &mut FxHashSet<ContainerID>) {
        for (items, _) in &self.stack {
            for item in items {
                out.extend(item.affected_cids.iter().cloned());
                for c in &item.meta.cursors {
                    out.insert(c.cursor.container.clone());
                }
            }
        }
    }
}

impl Default for Stack {
    fn default() -> Self {
        Stack::new()
    }
}

#[cfg(test)]
thread_local! {
    static UNDO_SIZE_WALKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl UndoManagerInner {
    fn new(last_counter: Counter) -> Self {
        Self {
            next_counter: Some(last_counter),
            undo_stack: Default::default(),
            redo_stack: Default::default(),
            processing_undo: false,
            processing_inverses: None,
            paused: false,
            merge_interval_in_ms: 0,
            last_undo_time: 0,
            max_stack_size: usize::MAX,
            max_undo_bytes: usize::MAX,
            exclude_origin_prefixes: vec![],
            last_popped_selection: None,
            on_pop: None,
            on_push: None,
            group: None,
        }
    }

    /// Returns true if a given container diff is disjoint with the current group.
    /// They are disjoint if they have no overlap in changed container ids.
    fn is_disjoint_with_group(&self, diff: &[&ContainerDiff]) -> bool {
        let Some(group) = &self.group else {
            return false;
        };

        diff.iter().all(|d| !group.affected_cids.contains(&d.id))
    }

    fn record_checkpoint(this: &RefCell<Self>, latest_counter: Counter, event: Option<DiffEvent>) {
        Self::record_checkpoint_at(this, latest_counter, event, None);
    }

    fn record_checkpoint_at(
        this: &RefCell<Self>,
        latest_counter: Counter,
        event: Option<DiffEvent>,
        now: Option<Timestamp>,
    ) {
        let previous_counter = this.borrow().next_counter;

        if Some(latest_counter) == previous_counter {
            return;
        }

        let Some(previous) = previous_counter else {
            this.borrow_mut().next_counter = Some(latest_counter);
            return;
        };

        let affected_cids: FxHashSet<ContainerID> = event
            .iter()
            .flat_map(|e| e.events.iter().map(|item| item.id.clone()))
            .collect();
        // Usable only if it covers exactly the ops of this checkpoint.
        let inverse = event
            .as_ref()
            .and_then(|e| e.event_meta.local_inverse.clone())
            .filter(|inv| inv.span.counter == CounterSpan::new(previous, latest_counter));

        if let Some(group) = &mut this.borrow_mut().group {
            group.affected_cids.extend(affected_cids.iter().cloned());
        }

        let now = now.unwrap_or_else(|| get_sys_timestamp() as Timestamp);
        let span = CounterSpan::new(previous, latest_counter);
        let meta = this
            .borrow()
            .on_push
            .as_ref()
            .map(|x| x(UndoOrRedo::Undo, span, event))
            .unwrap_or_default();

        let mut this = this.borrow_mut();
        let this: &mut Self = &mut this;
        // A backwards wall clock must not merge otherwise separate undo items.
        let elapsed = now - this.last_undo_time;
        let in_merge_interval = (0..this.merge_interval_in_ms).contains(&elapsed);

        // If group is active, but there is nothing in the group, don't merge
        // If the group is active and it's not the first push in the group, merge
        let group_should_merge = this.group.is_some()
            && match (
                previous_counter,
                this.group.as_ref().map(|g| g.start_counter),
            ) {
                (Some(previous), Some(active)) => previous != active,
                _ => true,
            };

        let should_merge = !this.undo_stack.is_empty() && (in_merge_interval || group_should_merge);

        let previous_len = this.undo_stack.len();
        if should_merge {
            this.undo_stack.push_with_merge_cids(
                span,
                meta,
                affected_cids,
                inverse,
                true,
                this.group.as_ref(),
            );
        } else {
            this.undo_stack
                .push_with_cids_and_inverse(span, meta, affected_cids, inverse);
        }
        // A requested merge can still create an item (e.g. after an import).
        // Only an actual merge keeps the previous item's window.
        if this.undo_stack.len() > previous_len {
            this.last_undo_time = now;
        }

        this.next_counter = Some(latest_counter);
        this.redo_stack.clear();
        this.trim_undo_stack();
    }

    pub(crate) fn used_bytes(&self) -> usize {
        #[cfg(test)]
        UNDO_SIZE_WALKS.with(|n| n.set(n.get() + 1));
        self.undo_stack
            .estimated_bytes()
            .saturating_add(self.redo_stack.estimated_bytes())
    }

    pub(crate) fn set_max_undo_bytes_from_doc(&mut self, max_bytes: usize) {
        self.max_undo_bytes = max_bytes;
        self.shrink_to_fit();
    }

    pub(crate) fn shrink_to_fit(&mut self) {
        if self.max_undo_bytes == usize::MAX {
            return;
        }
        let mut used_bytes = self.used_bytes();
        while used_bytes > self.max_undo_bytes {
            let freed = if !self.undo_stack.is_empty() {
                self.undo_stack.pop_front_inner(true)
            } else if !self.redo_stack.is_empty() {
                self.redo_stack.pop_front_inner(true)
            } else {
                break;
            };
            used_bytes = used_bytes.saturating_sub(freed);
        }
    }

    fn compose_remote_event(
        &mut self,
        diff: &[&ContainerDiff],
        remap: &FxHashMap<ContainerID, ContainerID>,
    ) {
        if self.undo_stack.is_empty() && self.redo_stack.is_empty() {
            return;
        }

        if self.undo_stack.has_unknown_affected_cids()
            || self.redo_stack.has_unknown_affected_cids()
        {
            self.undo_stack.compose_remote_event(diff);
            self.redo_stack.compose_remote_event(diff);
            return;
        }
        let mut relevant_cids: FxHashSet<ContainerID> = FxHashSet::default();
        self.undo_stack.collect_relevant_cids(&mut relevant_cids);
        self.redo_stack.collect_relevant_cids(&mut relevant_cids);
        if let Some(last_selection) = &self.last_popped_selection {
            for c in last_selection {
                relevant_cids.insert(c.cursor.container.clone());
            }
        }
        if let Some(group) = &self.group {
            relevant_cids.extend(group.affected_cids.iter().cloned());
        }
        for (old, new) in remap {
            relevant_cids.insert(old.clone());
            relevant_cids.insert(new.clone());
        }

        let filtered: SmallVec<[&ContainerDiff; 8]> = diff
            .iter()
            .copied()
            .filter(|e| matches!(e.diff, Diff::Tree(_)) || relevant_cids.contains(&e.id))
            .collect();
        if filtered.is_empty() {
            return;
        }

        self.undo_stack.compose_remote_event(&filtered);
        self.redo_stack.compose_remote_event(&filtered);
    }

    fn trim_undo_stack(&mut self) {
        while self.undo_stack.len() > self.max_stack_size {
            self.undo_stack.pop_front();
        }
        self.shrink_to_fit();
    }
}

fn get_counter_end(doc: &LoroDoc, peer: PeerID) -> Counter {
    doc.oplog().lock().vv().get(&peer).cloned().unwrap_or(0)
}

impl UndoManager {
    fn lock_inner(&self) -> MarkedUndoGuard<'_> {
        MarkedUndoGuard::lock(&self.inner, &self.doc.stats_dirty)
    }

    pub fn new(doc: &LoroDoc) -> Self {
        let peer = Arc::new(AtomicU64::new(doc.peer_id()));
        let peer_clone = peer.clone();
        let peer_clone2 = peer.clone();
        let inner = Arc::new(ReentrantMutex::new(RefCell::new({
            let mut u = UndoManagerInner::new(get_counter_end(doc, doc.peer_id()));
            if let Some(max_bytes) = doc.memory_budget().max_undo_bytes {
                u.max_undo_bytes = max_bytes;
            }
            u
        })));
        let inner_clone = inner.clone();
        let inner_clone2 = inner.clone();
        let dirty = doc.stats_dirty.clone();
        let dirty2 = doc.stats_dirty.clone();
        let remap_containers = Arc::new(Mutex::new(FxHashMap::default()));
        let remap_containers_local = remap_containers.clone();
        let remap_containers_clone = remap_containers.clone();
        let undo_sub = doc.subscribe_root(Arc::new(move |event| match event.event_meta.by {
            EventTriggerKind::Local => {
                let lock = MarkedUndoGuard::lock(&inner_clone, &dirty);
                if lock.borrow().processing_undo {
                    // The undo/redo's own commit: its inverse is the opposite
                    // stack's next item (see `perform`).
                    let mut inner = lock.borrow_mut();
                    let inverse = event.event_meta.local_inverse.clone();
                    inner.processing_inverses = match (inner.processing_inverses.take(), inverse) {
                        (Some(mut v), Some(inv)) => {
                            v.push(inv);
                            Some(v)
                        }
                        _ => None,
                    };
                    return;
                }
                if let Some(id) = event
                    .event_meta
                    .to
                    .iter()
                    .find(|x| x.peer == peer_clone.load(std::sync::atomic::Ordering::Relaxed))
                {
                    let is_paused = lock.borrow().paused;
                    let should_exclude = lock
                        .borrow()
                        .exclude_origin_prefixes
                        .iter()
                        .any(|x| event.event_meta.origin.starts_with(&**x));
                    if is_paused || should_exclude {
                        // Paused and excluded-origin edits are not recorded as
                        // undo steps, but their effects must be composed into
                        // both stacks so transforms stay correct, and
                        // next_counter must advance so the next recorded item
                        // does not fold these edits into its span.
                        let mut inner = lock.borrow_mut();
                        let remap = remap_containers_local.lock();
                        inner.compose_remote_event(event.events, &remap);
                        inner.next_counter = Some(id.counter + 1);
                    } else {
                        UndoManagerInner::record_checkpoint(&lock, id.counter + 1, Some(event));
                    }
                }
            }
            EventTriggerKind::Import => {
                let lock = MarkedUndoGuard::lock(&inner_clone, &dirty);
                let mut inner = lock.borrow_mut();

                {
                    let mut remap = remap_containers_clone.lock();
                    for e in event.events {
                        if let Diff::Tree(tree) = &e.diff {
                            for item in &tree.diff {
                                let target = item.target;
                                if let TreeExternalDiff::Create { .. } = &item.action {
                                    // If the concurrent event is a create event, it may bring the deleted tree node back,
                                    // so we need to remove it from the remap of the container.
                                    remap.remove(&target.associated_meta_container());
                                }
                            }
                        }
                    }

                    let is_import_disjoint = inner.is_disjoint_with_group(event.events);

                    inner.compose_remote_event(event.events, &remap);

                    // If the import is not disjoint, we end the active group
                    // all subsequent changes will be new undo items
                    if !is_import_disjoint {
                        inner.group = None;
                    }
                }
            }
            EventTriggerKind::Checkout => {
                let lock = MarkedUndoGuard::lock(&inner_clone, &dirty);
                if lock.borrow().paused {
                    return;
                }
                let mut inner = lock.borrow_mut();
                inner.undo_stack.clear();
                inner.redo_stack.clear();
                inner.next_counter = None;
            }
        }));

        let sub = doc.subscribe_peer_id_change(Box::new(move |id| {
            let lock = MarkedUndoGuard::lock(&inner_clone2, &dirty2);
            let mut inner = lock.borrow_mut();
            inner.undo_stack.clear();
            inner.redo_stack.clear();
            inner.next_counter = Some(id.counter);
            if inner.group.is_some() {
                // The stacks were cleared, so an active group starts over from
                // the new peer's counter, like a group started now.
                inner.group = Some(UndoGroup::new(id.counter));
            }
            peer_clone2.store(id.peer, std::sync::atomic::Ordering::Relaxed);
            true
        }));

        {
            let mut managers = doc.undo_managers.lock();
            managers.retain(|w| w.strong_count() > 0);
            managers.push(Arc::downgrade(&inner));
        }

        UndoManager {
            peer,
            container_remap: remap_containers,
            element_remap: Mutex::new(FxHashMap::default()),
            inner,
            _peer_id_change_sub: sub,
            _undo_sub: undo_sub,
            doc: doc.clone(),
            _stats_mark: StatsMarkOnDrop(doc.stats_dirty.clone()),
        }
    }

    pub fn set_max_undo_bytes(&self, max_bytes: usize) {
        let lock = self.lock_inner();
        if let Ok(mut inner) = lock.try_borrow_mut() {
            inner.max_undo_bytes = max_bytes;
            inner.shrink_to_fit();
        };
    }

    pub fn max_undo_bytes(&self) -> usize {
        let lock = self.lock_inner();
        let ans = lock
            .try_borrow()
            .map(|inner| inner.max_undo_bytes)
            .unwrap_or(usize::MAX);
        ans
    }

    pub fn used_bytes(&self) -> usize {
        let lock = self.lock_inner();
        let ans = lock
            .try_borrow()
            .map(|inner| inner.used_bytes())
            .unwrap_or(0);
        ans
    }

    pub fn shrink_to_fit(&self) {
        let lock = self.lock_inner();
        if let Ok(mut inner) = lock.try_borrow_mut() {
            inner.shrink_to_fit();
        };
    }

    pub fn group_start(&self) -> LoroResult<()> {
        let lock = self.lock_inner();
        let mut inner = lock.borrow_mut();

        if inner.group.is_some() {
            return Err(LoroError::UndoGroupAlreadyStarted);
        }

        inner.group =
            Some(UndoGroup::new(inner.next_counter.ok_or_else(|| {
                LoroError::Unknown("UndoManager is not ready".into())
            })?));

        Ok(())
    }

    pub fn group_end(&self) {
        self.lock_inner().borrow_mut().group = None;
    }

    pub fn peer(&self) -> PeerID {
        self.peer.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn set_merge_interval(&self, interval: i64) {
        self.lock_inner().borrow_mut().merge_interval_in_ms = interval;
    }

    pub fn set_max_undo_steps(&self, size: usize) {
        let inner = self.lock_inner();
        let mut inner = inner.borrow_mut();
        inner.max_stack_size = size;
        inner.trim_undo_stack();
    }

    pub fn add_exclude_origin_prefix(&self, prefix: &str) {
        self.inner
            .lock()
            .borrow_mut()
            .exclude_origin_prefixes
            .push(prefix.into());
    }

    pub fn record_new_checkpoint(&self) -> LoroResult<()> {
        // Use implicit-style barrier to preserve next-commit options across
        // an empty commit before undo/redo processing.
        self.doc.with_barrier(|| {});
        let counter = get_counter_end(&self.doc, self.peer());
        UndoManagerInner::record_checkpoint(&self.lock_inner(), counter, None);
        Ok(())
    }

    /// Consume at most one undo item, even if its transformed inverse is a no-op.
    /// Returns whether local operations were generated; use `can_undo` to check
    /// whether history remains after a `false` result.
    #[instrument(skip_all)]
    pub fn undo(&self) -> LoroResult<bool> {
        self.perform(
            |x| &mut x.undo_stack,
            |x| &mut x.redo_stack,
            UndoOrRedo::Undo,
        )
    }

    /// Consume at most one redo item, including a no-op. A `false` result means
    /// no local operations were generated, not necessarily an empty redo stack.
    #[instrument(skip_all)]
    pub fn redo(&self) -> LoroResult<bool> {
        self.perform(
            |x| &mut x.redo_stack,
            |x| &mut x.undo_stack,
            UndoOrRedo::Redo,
        )
    }

    fn perform(
        &self,
        get_stack: impl Fn(&mut UndoManagerInner) -> &mut Stack,
        get_opposite: impl Fn(&mut UndoManagerInner) -> &mut Stack,
        kind: UndoOrRedo,
    ) -> LoroResult<bool> {
        if self.lock_inner().borrow().paused {
            return Ok(false);
        }
        let doc = &self.doc.clone();
        let _exclusive = doc.txn.lock_exclusive();
        // When in the undo/redo loop, the new undo/redo stack item should restore the selection
        // to the state it was in before the item that was popped two steps ago from the stack.
        //
        //                          ┌────────────┐
        //                          │Selection 1 │
        //                          └─────┬──────┘
        //                                │   Some
        //                                ▼   ops
        //                          ┌────────────┐
        //                          │Selection 2 │
        //                          └─────┬──────┘
        //                                │   Some
        //                                ▼   ops
        //                          ┌────────────┐
        //                          │Selection 3 │◁ ─ ─ ─ ─ ─ ─ ─  Restore  ─ ─ ─
        //                          └─────┬──────┘                               │
        //                                │
        //                                │                                      │
        //                                │                              ┌ ─ ─ ─ ─ ─ ─ ─
        //           Enter the            │   Undo ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─▶   Push Redo   │
        //           undo/redo ─ ─ ─ ▶    ▼                              └ ─ ─ ─ ─ ─ ─ ─
        //             loop         ┌────────────┐                               │
        //                          │Selection 2 │◁─ ─ ─  Restore  ─
        //                          └─────┬──────┘                  │            │
        //                                │
        //                                │                         │            │
        //                                │                 ┌ ─ ─ ─ ─ ─ ─ ─
        //                                │   Undo ─ ─ ─ ─ ▶   Push Redo   │     │
        //                                ▼                 └ ─ ─ ─ ─ ─ ─ ─
        //                          ┌────────────┐                  │            │
        //                          │Selection 1 │
        //                          └─────┬──────┘                  │            │
        //                                │   Redo ◀ ─ ─ ─ ─ ─ ─ ─ ─
        //                                ▼                                      │
        //                          ┌────────────┐
        //         ┌   Restore   ─ ▷│Selection 2 │                               │
        //                          └─────┬──────┘
        //         │                      │                                      │
        // ┌ ─ ─ ─ ─ ─ ─ ─                │
        //    Push Undo   │◀─ ─ ─ ─ ─ ─ ─ │   Redo ◀ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘
        // └ ─ ─ ─ ─ ─ ─ ─                ▼
        //         │                ┌────────────┐
        //                          │Selection 3 │
        //         │                └─────┬──────┘
        //          ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ▶ │   Undo
        //                                ▼
        //                          ┌────────────┐
        //                          │Selection 2 │
        //                          └────────────┘
        //
        // Because users may change the selections during the undo/redo loop, it's
        // more stable to keep the selection stored in the last stack item
        // rather than using the current selection directly.
        self.record_new_checkpoint()?;
        let end_counter = get_counter_end(doc, self.peer());
        let top = {
            let lock = self.lock_inner();
            let mut inner = lock.borrow_mut();
            inner.processing_undo = true;
            inner.processing_inverses = Some(Vec::new());
            get_stack(&mut inner).pop()
        };
        let _guard = ProcessingUndoGuard {
            inner: self.inner.clone(),
        };

        let mut executed = false;
        // A no-op still consumes this item. Never cross its history boundary
        // to undo/redo an unrelated edit in the same call.
        if let Some((mut span, remote_diff)) = top {
            let mut next_push_selection = None;
            let mut next_affected_cids = std::mem::take(&mut span.affected_cids);
            {
                let inner = self.inner.clone();
                // We need to clone this because otherwise <transform_delta> will be applied to the same remote diff
                let remote_change_clone = remote_diff.lock().clone();
                let mut rejected_step = None;
                let item_inverse = span.inverse.clone();
                let commit = match doc.undo_internal_with(
                    IdSpan {
                        peer: self.peer(),
                        counter: span.span,
                    },
                    item_inverse.as_deref(),
                    &mut self.container_remap.lock(),
                    Some(&remote_change_clone),
                    &mut |diff| {
                        next_affected_cids.extend(diff.cid_to_events.keys().cloned());
                        info_span!("transform remote diff").in_scope(|| {
                            let inner = inner.lock();
                            // <transform_delta>
                            get_stack(&mut inner.borrow_mut()).transform_based_on_this_delta(diff);
                        });
                    },
                    &mut rejected_step,
                    &mut self.element_remap.lock(),
                ) {
                    Ok(c) => c,
                    // Keep the documented unknown-container policy: consume a
                    // prechecked unsupported step and rebase older history.
                    // Application errors, including other ArgErrs, are retried.
                    Err(e @ LoroError::ArgErr(_)) if rejected_step.is_some() => {
                        // Its changes stay in the doc: rebase the steps before
                        // it over them, like over remote changes
                        if let Some(mut changes) = rejected_step {
                            let mut remote = remote_diff.lock();
                            changes.compose(&remote);
                            *remote = changes;
                        }
                        return Err(e);
                    }
                    Err(e) => {
                        span.affected_cids = next_affected_cids;
                        get_stack(&mut self.lock_inner().borrow_mut())
                            .restore_popped(span, &remote_diff);
                        return Err(e);
                    }
                };
                {
                    let remap = self.container_remap.lock();
                    let extra: Vec<ContainerID> = next_affected_cids
                        .iter()
                        .filter_map(|cid| {
                            let mut cur = cid;
                            let mut mapped = None;
                            while let Some(next) = remap.get(cur) {
                                mapped = Some(next.clone());
                                cur = next;
                            }
                            mapped
                        })
                        .collect();
                    next_affected_cids.extend(extra);
                }
                drop(commit);
                let inner = self.lock_inner();
                let mut is_some = false;

                if let Some(on_pop) = inner.borrow().on_pop.as_ref() {
                    is_some = true;
                    for cursor in span.meta.cursors.iter_mut() {
                        // <cursor_transform> We need to transform cursor here.
                        // Note that right now <transform_delta> is already done,
                        // remote_diff is also transformed by it now (that's what we need).
                        transform_cursor(
                            cursor,
                            &remote_diff.lock(),
                            doc,
                            &self.container_remap.lock(),
                        );
                    }

                    on_pop(kind, span.span, span.meta.clone());
                }
                if is_some {
                    let take = inner.borrow_mut().last_popped_selection.take();
                    next_push_selection = take;
                    inner.borrow_mut().last_popped_selection = Some(span.meta.cursors);
                }
            }
            let new_counter = get_counter_end(doc, self.peer());
            if end_counter != new_counter {
                let inner = self.lock_inner();
                let mut meta = inner
                    .borrow()
                    .on_push
                    .as_ref()
                    .map(|x| {
                        x(
                            kind.opposite(),
                            CounterSpan::new(end_counter, new_counter),
                            None,
                        )
                    })
                    .unwrap_or_default();

                if matches!(kind, UndoOrRedo::Undo)
                    && get_opposite(&mut inner.borrow_mut()).is_empty()
                {
                    // If it's the first undo, we use the cursors from the users
                } else if let Some(inner) = next_push_selection.take() {
                    // Otherwise, we use the cursors from the undo/redo loop
                    meta.cursors = inner;
                }

                let mut inner = inner.borrow_mut();
                // The inverse of the commit(s) this undo/redo just made.
                let opposite_inverse = inner
                    .processing_inverses
                    .take()
                    .and_then(|v| {
                        let mut it = v.into_iter();
                        let first = it.next();
                        it.fold(first, |acc, next| LocalInverse::chain(acc, Some(&next)))
                    })
                    .filter(|inv| inv.span.counter == CounterSpan::new(end_counter, new_counter));
                get_opposite(&mut inner).push_with_cids_and_inverse(
                    CounterSpan::new(end_counter, new_counter),
                    meta,
                    next_affected_cids,
                    opposite_inverse,
                );
                inner.trim_undo_stack();
                inner.next_counter = Some(new_counter);
                executed = true;
            }
        }

        Ok(executed)
    }

    pub fn can_undo(&self) -> bool {
        !self.lock_inner().borrow().undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.lock_inner().borrow().redo_stack.is_empty()
    }

    pub fn undo_count(&self) -> usize {
        self.lock_inner().borrow().undo_stack.len()
    }

    pub fn redo_count(&self) -> usize {
        self.lock_inner().borrow().redo_stack.len()
    }

    /// Get the metadata of the top undo stack item, if any.
    pub fn top_undo_meta(&self) -> Option<UndoItemMeta> {
        self.lock_inner().borrow().undo_stack.peek_top_meta()
    }

    /// Get the metadata of the top redo stack item, if any.
    pub fn top_redo_meta(&self) -> Option<UndoItemMeta> {
        self.lock_inner().borrow().redo_stack.peek_top_meta()
    }

    /// Get the counter span of the top undo stack item, if any.
    ///
    /// A span that equals the one passed to `on_push` identifies the pushed item, also when
    /// the push evicted the oldest item at the stack size limit. A push merged into the
    /// previous item extends that item's span instead.
    pub fn top_undo_span(&self) -> Option<CounterSpan> {
        self.lock_inner().borrow().undo_stack.peek_top_span()
    }

    /// Get the counter span of the top redo stack item, if any.
    pub fn top_redo_span(&self) -> Option<CounterSpan> {
        self.lock_inner().borrow().redo_stack.peek_top_span()
    }

    /// Get the value associated with the top undo stack item, if any.
    pub fn top_undo_value(&self) -> Option<LoroValue> {
        self.top_undo_meta().map(|m| m.value)
    }

    /// Get the value associated with the top redo stack item, if any.
    pub fn top_redo_value(&self) -> Option<LoroValue> {
        self.top_redo_meta().map(|m| m.value)
    }

    pub fn set_on_push(&self, on_push: Option<OnPush>) {
        self.lock_inner().borrow_mut().on_push = on_push;
    }

    pub fn set_on_pop(&self, on_pop: Option<OnPop>) {
        self.lock_inner().borrow_mut().on_pop = on_pop;
    }

    pub fn clear(&self) {
        self.lock_inner().borrow_mut().undo_stack.clear();
        self.lock_inner().borrow_mut().redo_stack.clear();
    }

    /// Clear only the redo stack, preserving the undo stack.
    pub fn clear_redo(&self) {
        self.lock_inner().borrow_mut().redo_stack.clear();
    }

    /// Clear only the undo stack, preserving the redo stack.
    pub fn clear_undo(&self) {
        self.lock_inner().borrow_mut().undo_stack.clear();
    }

    /// Pause the UndoManager so that local edits and checkout events are ignored.
    ///
    /// While paused, local edits are not recorded as undo steps and checkout
    /// events do not clear the stacks. Import events (remote changes) are still
    /// processed so that the stacks remain correctly transformed against
    /// concurrent edits.
    ///
    /// Use this before temporary checkouts (e.g. read-only history preview) that
    /// should not disturb the undo/redo history. Close any open group (via
    /// [`Self::group_end`]) before pausing.
    ///
    /// Call [`Self::resume`] after returning the document to its original state.
    pub fn pause(&self) {
        self.lock_inner().borrow_mut().paused = true;
    }

    /// Resume the UndoManager after a [`Self::pause`].
    pub fn resume(&self) {
        self.lock_inner().borrow_mut().paused = false;
    }

    /// Returns whether the UndoManager is currently paused.
    pub fn is_paused(&self) -> bool {
        self.lock_inner().borrow().paused
    }

    pub fn set_top_undo_meta(&self, meta: UndoItemMeta) {
        self.lock_inner().borrow_mut().undo_stack.set_top_meta(meta);
    }

    pub fn set_top_redo_meta(&self, meta: UndoItemMeta) {
        self.lock_inner().borrow_mut().redo_stack.set_top_meta(meta);
    }
}

/// Undo the given spans of operations.
///
/// # Parameters
///
/// - `spans`: A vector of tuples where each tuple contains an `IdSpan` and its associated `Frontiers`.
///   - `IdSpan`: Represents a span of operations identified by an ID.
///   - `Frontiers`: Represents the deps of the given id_span
/// - `last_frontiers_or_last_bi`: The latest frontiers, or the precomputed final forward diff
/// - `calc_diff`: A closure that takes two `Frontiers` and calculates the difference between them, returning a `DiffBatch`.
///
/// # Returns
///
/// - `DiffBatch`: Applying this batch on the `latest_frontiers` will undo the ops in the given spans.
pub(crate) fn undo(
    spans: Vec<(IdSpan, Frontiers)>,
    last_frontiers_or_last_bi: Either<&Frontiers, &DiffBatch>,
    calc_diff: impl Fn(&Frontiers, &Frontiers) -> DiffBatch,
    on_last_event_a: &mut dyn FnMut(&DiffBatch),
) -> DiffBatch {
    // The process of performing undo is:
    //
    // 0. Split the span into a series of continuous spans. There is no external dep within each continuous span.
    //
    // For each continuous span_i:
    //
    // 1. a. Calculate the event of checkout from id_span.last to id_span.deps, call it Ai. It undo the ops in the current span.
    //    b. Calculate A'i = Ai + Ci-1 if i > 0, otherwise A'i = Ai.
    //       Ci-1 is based at this span's deps, exactly where Ai ends, so compose directly.
    //       NOTE: A'i can undo the ops in the current span and the previous spans, if it's applied on the id_span.last version.
    // 2. Calculate the checkout from id_span.last to the next span's deps, call it Bi.
    //    These deps include this span's last op because a peer's ops are causally ordered.
    //    Bi crosses only the other peers' ops between spans. Taking Ci through the next
    //    span's own ops and back through their inverse is lossy: restored runs can be
    //    reordered (#1245), or intermediate text can survive the undo (#1228).
    //    For the last span only, Bi goes to the latest version (or uses the supplied diff).
    // 3. Transform event A'i based on Bi, call it Ci
    // 4. If span_i is the last span, apply Ci to the current state.

    // -------------------------------------------------------
    // 0. Split the span into a series of continuous spans
    // -------------------------------------------------------

    let mut last_ci: Option<DiffBatch> = None;
    for i in 0..spans.len() {
        debug_span!("Undo", ?i, "Undo span {:?}", &spans[i]).in_scope(|| {
            let (this_id_span, this_deps) = &spans[i];
            // ---------------------------------------
            // 1.a Calc event A_i
            // ---------------------------------------
            let mut event_a_i = debug_span!("1. Calc event A_i").in_scope(|| {
                // Checkout to the last id of the id_span
                calc_diff(&this_id_span.id_last().into(), this_deps)
            });

            // println!("event_a_i: {:?}", event_a_i);

            // ---------------------------------------
            // 2. Calc event B_i
            // ---------------------------------------
            let stack_diff_batch;
            let event_b_i = 'block: {
                let next = if i + 1 < spans.len() {
                    &spans[i + 1].1
                } else {
                    match last_frontiers_or_last_bi {
                        Either::Left(last_frontiers) => last_frontiers,
                        Either::Right(right) => break 'block Some(right),
                    }
                };
                let from = Frontiers::from_id(this_id_span.id_last());
                if &from == next {
                    break 'block None;
                }
                stack_diff_batch = calc_diff(&from, next);
                Some(&stack_diff_batch)
            };

            // println!("event_b_i: {:?}", event_b_i);

            // event_a_prime can undo the ops in the current span and the previous spans
            let mut event_a_prime = if let Some(last_ci) = last_ci.take() {
                // ------------------------------------------------------------------------------
                // 1.b Apply Ci-1 after Ai, call it A'i
                // ------------------------------------------------------------------------------
                event_a_i.compose_inverse(last_ci);
                event_a_i
            } else {
                event_a_i
            };
            if i == spans.len() - 1 {
                on_last_event_a(&event_a_prime);
            }
            // --------------------------------------------------
            // 3. Transform event A'_i based on B_i, call it C_i
            // --------------------------------------------------
            if let Some(event_b_i) = event_b_i {
                event_a_prime.transform(event_b_i, true);
            }

            // println!("event_a_prime: {:?}", event_a_prime);

            let c_i = event_a_prime;
            last_ci = Some(c_i);
        });
    }

    last_ci.unwrap()
}

#[cfg(test)]
mod merge_timer_tests {
    use super::*;
    use loro_common::ContainerType;

    fn manager() -> RefCell<UndoManagerInner> {
        let mut inner = UndoManagerInner::new(0);
        inner.merge_interval_in_ms = 500;
        RefCell::new(inner)
    }

    fn record(inner: &RefCell<UndoManagerInner>, counter: Counter, now: Timestamp) {
        UndoManagerInner::record_checkpoint_at(inner, counter, None, Some(now));
    }

    #[test]
    fn first_item_starts_its_own_merge_window() {
        let inner = manager();
        record(&inner, 1, 100);
        assert_eq!(inner.borrow().last_undo_time, 100);
        record(&inner, 2, 550);
        assert_eq!(inner.borrow().undo_stack.len(), 1);
    }

    #[test]
    fn merged_edits_keep_the_original_window() {
        let inner = manager();
        record(&inner, 1, 100_000);
        record(&inner, 2, 100_300);
        assert_eq!(inner.borrow().undo_stack.len(), 1);
        assert_eq!(inner.borrow().last_undo_time, 100_000);
        record(&inner, 3, 100_600);
        assert_eq!(inner.borrow().undo_stack.len(), 2);
        assert_eq!(inner.borrow().last_undo_time, 100_600);
    }

    #[test]
    fn remote_diff_starts_a_new_merge_window() {
        let inner = manager();
        record(&inner, 1, 100_000);
        // A remote import leaves a diff on the row containing the old item.
        let mut diff = crate::event::TextDiff::default();
        diff.insert_value(0, "remote".into(), Default::default());
        {
            let inner = inner.borrow();
            let mut remote_diff = inner.undo_stack.stack.back().unwrap().1.lock();
            let cid = ContainerID::new_root("text", ContainerType::Text);
            remote_diff
                .cid_to_events
                .insert(cid.clone(), Diff::Text(diff));
            remote_diff.order.push(cid);
        }
        record(&inner, 2, 100_300);
        assert_eq!(inner.borrow().undo_stack.len(), 2);
        assert_eq!(inner.borrow().last_undo_time, 100_300);
        record(&inner, 3, 100_600);
        let inner = inner.borrow();
        assert_eq!(inner.undo_stack.len(), 2);
        assert_eq!(
            inner.undo_stack.peek_top_span(),
            Some(CounterSpan::new(1, 3))
        );
    }

    #[test]
    fn noncontiguous_span_starts_a_new_merge_window() {
        let inner = manager();
        record(&inner, 1, 100_000);
        inner.borrow_mut().next_counter = Some(2);
        record(&inner, 3, 100_300);
        assert_eq!(inner.borrow().last_undo_time, 100_300);
        record(&inner, 4, 100_600);
        let inner = inner.borrow();
        assert_eq!(inner.undo_stack.len(), 2);
        assert_eq!(
            inner.undo_stack.peek_top_span(),
            Some(CounterSpan::new(2, 4))
        );
    }

    #[test]
    fn empty_tail_row_starts_a_new_merge_window() {
        let inner = manager();
        record(&inner, 1, 100_000);
        record(&inner, 2, 100_600);
        inner
            .borrow_mut()
            .undo_stack
            .stack
            .push_back(Default::default());
        record(&inner, 3, 100_900);
        assert_eq!(inner.borrow().last_undo_time, 100_900);
        record(&inner, 4, 101_200);
        let inner = inner.borrow();
        assert_eq!(inner.undo_stack.len(), 3);
        assert_eq!(
            inner.undo_stack.peek_top_span(),
            Some(CounterSpan::new(2, 4))
        );
    }
}

#[cfg(test)]
mod budget_work_tests {
    use super::*;
    #[test]
    fn finite_byte_trim_walks_history_once_and_accounts_removed_rows() {
        let mut manager = UndoManagerInner::new(0);
        for i in 0..1000 {
            manager.undo_stack.push_with_cids(
                CounterSpan::new(i, i + 1),
                UndoItemMeta::default(),
                Default::default(),
            );
            if i % 7 == 0 {
                manager.undo_stack.stack.push_back(Default::default());
            }
        }
        UNDO_SIZE_WALKS.with(|n| n.set(0));
        manager.set_max_undo_bytes_from_doc(0);
        UNDO_SIZE_WALKS.with(|n| assert_eq!(n.get(), 1));
        assert!(manager.undo_stack.is_empty());

        let mut stack = Stack::new();
        for i in 0..20 {
            stack.push_with_cids(
                CounterSpan::new(i, i + 1),
                UndoItemMeta {
                    value: LoroValue::from("payload".repeat(i as usize + 1)),
                    cursors: vec![],
                },
                Default::default(),
            );
            if i % 3 == 0 {
                let mut diff = crate::event::TextDiff::default();
                diff.insert_value(0, "remote".into(), Default::default());
                let cid = ContainerID::new_root("text", loro_common::ContainerType::Text);
                let batch = &stack.stack.back().unwrap().1;
                batch.lock().order.push(cid.clone());
                batch.lock().cid_to_events.insert(cid, Diff::Text(diff));
                stack.stack.push_back(Default::default());
            }
        }
        while !stack.is_empty() {
            let before = stack.estimated_bytes();
            let freed = stack.pop_front_inner(true);
            assert_eq!(before - freed, stack.estimated_bytes());
        }
    }

    #[test]
    fn merging_known_edits_does_not_narrow_an_unknown_checkpoint() {
        let mut stack = Stack::new();
        stack.push_with_cids(
            CounterSpan::new(0, 1),
            Default::default(),
            Default::default(),
        );
        let cid = ContainerID::new_root("text", loro_common::ContainerType::Text);
        stack.push_with_merge_cids(
            CounterSpan::new(1, 2),
            Default::default(),
            [cid].into_iter().collect(),
            None,
            true,
            None,
        );
        assert_eq!(stack.len(), 1);
        assert!(stack.has_unknown_affected_cids());
    }

    #[test]
    fn unlimited_undo_trimming_does_not_walk_history() {
        let mut manager = UndoManagerInner::new(0);
        for i in 0..1000 {
            manager.undo_stack.push_with_cids(
                CounterSpan::new(i, i + 1),
                UndoItemMeta::default(),
                Default::default(),
            );
        }
        UNDO_SIZE_WALKS.with(|n| n.set(0));
        manager.trim_undo_stack();
        assert_eq!(manager.undo_stack.len(), 1000);
        UNDO_SIZE_WALKS.with(|n| assert_eq!(n.get(), 0));
        manager.set_max_undo_bytes_from_doc(0);
        assert!(manager.undo_stack.is_empty());
    }
}

/// Whether undo also checks the document out for steps that have a recorded
/// inverse, and asserts both agree: always in this crate's tests, and in debug
/// builds with `LORO_VERIFY_UNDO_LOG` set.
pub(crate) fn verify_recorded_inverses() -> bool {
    #[cfg(test)]
    return !SKIP_VERIFY.with(|s| s.get());
    #[cfg(not(test))]
    {
        if !cfg!(debug_assertions) {
            return false;
        }
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ON.get_or_init(|| std::env::var_os("LORO_VERIFY_UNDO_LOG").is_some())
    }
}

/// Panics unless the two batches mean the same changes: map entries in any
/// order, empty diffs ignored, and each text/list delta compared as its
/// sequence of retains, deletes and inserts with adjacent items of the same
/// kind merged. A checkout splits inserted text at chunk boundaries and may
/// keep a delete and the insert after it apart; both apply and transform the
/// same.
pub(crate) fn assert_same_undo_diff(from_checkout: &DiffBatch, recorded: &DiffBatch) {
    #[derive(Debug, PartialEq)]
    enum Op {
        Retain(usize, String),
        Delete(usize),
        Insert(String, String),
    }
    fn push(ops: &mut Vec<Op>, op: Op) {
        match (ops.last_mut(), op) {
            (Some(Op::Retain(n, a)), Op::Retain(m, b)) if *a == b => *n += m,
            (Some(Op::Delete(n)), Op::Delete(m)) => *n += m,
            (Some(Op::Insert(s, a)), Op::Insert(t, b)) if *a == b => s.push_str(&t),
            (_, op) => {
                if !matches!(op, Op::Retain(0, _) | Op::Delete(0))
                    && !matches!(&op, Op::Insert(s, _) if s.is_empty())
                {
                    ops.push(op)
                }
            }
        }
    }
    fn delta_ops<A: loro_delta::delta_trait::DeltaAttr>(
        items: impl Iterator<Item = (Option<usize>, usize, String, A)>,
    ) -> Vec<Op> {
        let mut ops = Vec::new();
        for (retain, delete, insert, attr) in items {
            // Empty attributes as "", so trailing plain retains can be dropped.
            let attr = if attr.attr_is_empty() {
                String::new()
            } else {
                format!("{attr:?}")
            };
            if let Some(n) = retain {
                push(&mut ops, Op::Retain(n, attr));
                continue;
            }
            push(&mut ops, Op::Delete(delete));
            push(&mut ops, Op::Insert(insert, attr));
        }
        while matches!(ops.last(), Some(Op::Retain(_, a)) if a.is_empty()) {
            ops.pop();
        }
        ops
    }
    fn canonical(batch: &DiffBatch) -> std::collections::BTreeMap<String, String> {
        batch
            .cid_to_events
            .iter()
            .filter(|(_, d)| !d.is_empty())
            .map(|(cid, d)| {
                let s = match d {
                    Diff::Map(m) => {
                        let entries: std::collections::BTreeMap<_, _> = m
                            .updated
                            .iter()
                            .map(|(k, v)| (k.to_string(), format!("{v:?}")))
                            .collect();
                        format!("Map({entries:?})")
                    }
                    Diff::Text(t) => format!(
                        "Text({:?})",
                        delta_ops(t.iter().map(|item| match item {
                            loro_delta::DeltaItem::Retain { len, attr } =>
                                (Some(*len), 0, String::new(), attr.clone()),
                            loro_delta::DeltaItem::Replace {
                                value,
                                attr,
                                delete,
                            } => (None, *delete, value.as_str().to_string(), attr.clone()),
                        }))
                    ),
                    Diff::List(l) => format!(
                        "List({:?})",
                        delta_ops(l.iter().map(|item| match item {
                            loro_delta::DeltaItem::Retain { len, attr } =>
                                (Some(*len), 0, String::new(), *attr),
                            loro_delta::DeltaItem::Replace {
                                value,
                                attr,
                                delete,
                            } => (
                                None,
                                *delete,
                                value.iter().map(|v| format!("{v:?};")).collect::<String>(),
                                *attr,
                            ),
                        }))
                    ),
                    d => format!("{d:?}"),
                };
                (format!("{cid:?}"), s)
            })
            .collect()
    }
    let (a, b) = (canonical(from_checkout), canonical(recorded));
    assert_eq!(
        a, b,
        "the recorded inverse differs from the checkout's (left: checkout, right: recorded)"
    );
}

#[cfg(test)]
thread_local! {
    /// Turns the checkout comparison off, to observe undo without checkouts.
    pub(crate) static SKIP_VERIFY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Undo steps that used a recorded inverse instead of checking out.
    pub(crate) static RECORDED_INVERSES_USED: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
mod recorded_inverse_tests {
    use super::*;
    use crate::{cursor::PosType, loro::ExportMode, LoroDoc};

    /// Tree creations and moves between live parents (with a remote peer's
    /// concurrent tree edits in between) undo and redo from recorded inverses:
    /// with the checkout comparison on, every step is compared; with it off,
    /// no diff is computed.
    #[test]
    fn tree_creations_and_moves_undo_from_recorded_inverses() {
        use crate::TreeParentId;
        for verify in [true, false] {
            let doc = LoroDoc::new_auto_commit();
            doc.set_peer_id(1).unwrap();
            let remote = LoroDoc::new_auto_commit();
            remote.set_peer_id(2).unwrap();
            let tree = doc.get_tree("tree");
            let mut nodes = Vec::new();
            for _ in 0..4 {
                nodes.push(tree.create(TreeParentId::Root).unwrap());
            }
            doc.commit_then_renew();
            let undo = UndoManager::new(&doc);
            undo.set_merge_interval(0);
            let original = doc.get_deep_value();
            let sync = || {
                remote
                    .import(&doc.export(ExportMode::updates(&remote.oplog_vv())).unwrap())
                    .unwrap();
                let rtree = remote.get_tree("tree");
                rtree.create(TreeParentId::Root).unwrap();
                remote.commit_then_renew();
                doc.import(&remote.export(ExportMode::updates(&doc.oplog_vv())).unwrap())
                    .unwrap();
            };
            let child = tree.create(TreeParentId::Node(nodes[0])).unwrap();
            doc.commit_then_renew();
            sync();
            tree.mov(nodes[1], TreeParentId::Node(nodes[2])).unwrap();
            doc.commit_then_renew();
            tree.mov(child, TreeParentId::Root).unwrap();
            doc.commit_then_renew();
            sync();
            tree.move_to(nodes[3], TreeParentId::Root, 0).unwrap();
            doc.commit_then_renew();
            let steps = undo.undo_count();
            assert_eq!(steps, 4);

            SKIP_VERIFY.with(|s| s.set(!verify));
            RECORDED_INVERSES_USED.with(|n| n.set(0));
            crate::diff_calc::DIFF_CALCS.with(|n| n.set(0));
            for _ in 0..steps {
                assert!(undo.undo().unwrap());
            }
            let undone = doc.get_deep_value();
            for _ in 0..steps {
                assert!(undo.redo().unwrap());
            }
            SKIP_VERIFY.with(|s| s.set(false));
            assert_eq!(
                RECORDED_INVERSES_USED.with(|n| n.get()),
                2 * steps,
                "verify {verify}"
            );
            if !verify {
                assert_eq!(crate::diff_calc::DIFF_CALCS.with(|n| n.get()), 0);
            }
            // Only the local nodes' places are undone; the remote nodes stay.
            let roots = |v: &loro_common::LoroValue| match v {
                loro_common::LoroValue::Map(m) => match m.get("tree") {
                    Some(loro_common::LoroValue::List(l)) => l.len(),
                    _ => 0,
                },
                _ => 0,
            };
            assert_eq!(
                roots(&undone),
                roots(&original) + 2,
                "two remote roots stay"
            );
        }
    }

    /// Movable-list inserts, deletes, moves and sets (one op per step, with a
    /// remote peer's concurrent edits) undo and redo from recorded identity
    /// snapshots: compared with the checkout's when verifying, and with no diff
    /// computed otherwise.
    #[test]
    fn movable_list_steps_undo_from_recorded_inverses() {
        for verify in [true, false] {
            let doc = LoroDoc::new_auto_commit();
            doc.set_peer_id(1).unwrap();
            let remote = LoroDoc::new_auto_commit();
            remote.set_peer_id(2).unwrap();
            let list = doc.get_movable_list("list");
            for i in 0..20 {
                list.push(LoroValue::I64(i)).unwrap();
            }
            doc.commit_then_renew();
            let sync = || {
                remote
                    .import(&doc.export(ExportMode::updates(&remote.oplog_vv())).unwrap())
                    .unwrap();
                let r = remote.get_movable_list("list");
                r.insert(0, -1).unwrap();
                r.mov(r.len() - 1, 1).unwrap();
                remote.commit_then_renew();
                doc.import(&remote.export(ExportMode::updates(&doc.oplog_vv())).unwrap())
                    .unwrap();
            };
            sync();
            let undo = UndoManager::new(&doc);
            undo.set_merge_interval(0);
            list.insert(3, 100).unwrap();
            doc.commit_then_renew();
            list.delete(5, 3).unwrap();
            doc.commit_then_renew();
            sync();
            list.mov(2, 10).unwrap();
            doc.commit_then_renew();
            list.set(4, "set").unwrap();
            doc.commit_then_renew();
            sync();
            let steps = undo.undo_count();
            assert_eq!(steps, 4);
            let edited = doc.get_deep_value();

            SKIP_VERIFY.with(|s| s.set(!verify));
            RECORDED_INVERSES_USED.with(|n| n.set(0));
            crate::diff_calc::DIFF_CALCS.with(|n| n.set(0));
            for _ in 0..steps {
                assert!(undo.undo().unwrap());
            }
            for _ in 0..steps {
                assert!(undo.redo().unwrap());
            }
            SKIP_VERIFY.with(|s| s.set(false));
            assert_eq!(
                RECORDED_INVERSES_USED.with(|n| n.get()),
                2 * steps,
                "verify {verify}"
            );
            if !verify {
                assert_eq!(crate::diff_calc::DIFF_CALCS.with(|n| n.get()), 0);
            }
            assert_eq!(doc.get_deep_value(), edited);
            remote
                .import(&doc.export(ExportMode::updates(&remote.oplog_vv())).unwrap())
                .unwrap();
            assert_eq!(remote.get_deep_value(), doc.get_deep_value());
        }
    }

    /// Undo and redo of text, list, map and counter edits on a document opened
    /// from a snapshot compute no diff, so they build no history cache, parsed
    /// history or diff tracker (the first undo used to, and they stayed).
    #[test]
    fn undo_with_recorded_inverses_builds_no_history_state() {
        let src = LoroDoc::new_auto_commit();
        src.set_peer_id(1).unwrap();
        let text = src.get_text("t");
        for i in 0..200 {
            text.insert(text.len_unicode(), &format!("{i:09} "), PosType::Unicode)
                .unwrap();
            src.get_map("m").insert(&format!("k{i}"), i).unwrap();
            src.get_list("l").push(i).unwrap();
            src.commit_then_renew();
        }
        let snapshot = src.export(ExportMode::Snapshot).unwrap();
        let doc = LoroDoc::new_auto_commit();
        doc.import(&snapshot).unwrap();
        doc.set_peer_id(9).unwrap();
        let undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);
        // A root acquired through a handle stays visible (empty) after undo
        // (context/root-container-visibility.md), so acquire it first.
        #[cfg(feature = "counter")]
        let counter = doc.get_counter("c");
        let original = doc.get_deep_value();
        let text = doc.get_text("t");
        text.insert(5, "hello", PosType::Unicode).unwrap();
        doc.commit_then_renew();
        text.delete(20, 30, PosType::Unicode).unwrap();
        doc.commit_then_renew();
        doc.get_map("m").insert("k3", "changed").unwrap();
        doc.get_map("m").insert("new", 1).unwrap();
        doc.commit_then_renew();
        doc.get_list("l").delete(10, 5).unwrap();
        doc.get_list("l").insert(0, "front").unwrap();
        doc.commit_then_renew();
        #[cfg(feature = "counter")]
        {
            counter.increment(2.5).unwrap();
            doc.commit_then_renew();
        }
        let edited = doc.get_deep_value();
        let steps = undo.undo_count();
        let before = doc.memory_stats();

        SKIP_VERIFY.with(|s| s.set(true));
        RECORDED_INVERSES_USED.with(|n| n.set(0));
        crate::diff_calc::DIFF_CALCS.with(|n| n.set(0));
        for _ in 0..steps {
            assert!(undo.undo().unwrap());
        }
        assert_eq!(doc.get_deep_value(), original);
        for _ in 0..steps {
            assert!(undo.redo().unwrap());
        }
        assert_eq!(doc.get_deep_value(), edited);
        let after = doc.memory_stats();
        SKIP_VERIFY.with(|s| s.set(false));

        assert_eq!(RECORDED_INVERSES_USED.with(|n| n.get()), 2 * steps);
        assert_eq!(crate::diff_calc::DIFF_CALCS.with(|n| n.get()), 0);
        assert_eq!(after.history_cache_bytes, before.history_cache_bytes);
        assert_eq!(after.diff_calc_bytes, before.diff_calc_bytes);
    }
}
