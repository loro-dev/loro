use std::{
    borrow::Cow,
    mem::take,
    sync::{Arc, Weak},
};

use enum_as_inner::EnumAsInner;
use generic_btree::rle::{HasLength as RleHasLength, Mergeable as GBSliceable};
use loro_common::{ContainerType, IdLp, IdSpan, LoroResult};
use loro_delta::{array_vec::ArrayVec, DeltaRopeBuilder};
use rle::{HasLength, Mergable, RleVec, Sliceable};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::utils::subscription::DeferredPanic;
use crate::{
    change::{Change, Lamport, Timestamp},
    container::{
        idx::ContainerIdx,
        list::list_op::{DeleteSpan, InnerListOp},
        richtext::Style,
        IntoContainerId,
    },
    delta::{ResolvedMapDelta, ResolvedMapValue, StyleMeta, StyleMetaItem, TreeDiff, TreeDiffItem},
    encoding::export_fast_updates_in_range,
    event::{Diff, DocDiff, ListDeltaMeta, TextDiff},
    handler::{Handler, ValueOrHandler},
    id::{Counter, PeerID, ID},
    lock::LoroMutexGuard,
    loro::CommitOptions,
    op::{InnerContent, Op, RawOp, RawOpContent},
    pre_commit::{ChangeModifier, PreCommitCallbackPayload},
    span::HasIdSpan,
    version::Frontiers,
    ChangeMeta, InternalString, LoroDoc, LoroDocInner, LoroError, LoroValue,
};

use super::{
    arena::SharedArena,
    event::{InternalContainerDiff, InternalDocDiff},
    handler::{ListHandler, MapHandler, TextHandler, TreeHandler},
    state::DocState,
};

#[derive(Default)]
pub(crate) struct PostCommit {
    pub events: Vec<DocDiff>,
    local_update: Option<Vec<u8>>,
}

impl crate::LoroDoc {
    /// Create a new transaction.
    /// Every ops created inside one transaction will be packed into a single
    /// [Change].
    ///
    /// There can only be one active transaction at a time for a [LoroDoc].
    #[inline(always)]
    pub fn txn(&self) -> Result<Transaction, LoroError> {
        self.txn_with_origin("")
    }

    /// Create a new transaction with specified origin.
    ///
    /// The origin will be propagated to the events.
    /// There can only be one active transaction at a time for a [LoroDoc].
    ///
    /// It waits for the doc's transaction lock, like every import and auto-commit
    /// transaction: a transaction must not start while an import runs, including
    /// between the blobs of an [`LoroDoc::import_batch`]. See
    /// `context/import-peer-id-reuse.md`.
    ///
    /// Called back on a thread that holds the lock, as in an event or pre-commit
    /// callback, it returns [`LoroError::DuplicatedTransactionError`]: a transaction
    /// or an import is running there.
    pub fn txn_with_origin(&self, origin: &str) -> Result<Transaction, LoroError> {
        if self.txn.is_held_by_current_thread() {
            return Err(LoroError::DuplicatedTransactionError);
        }
        let _guard = self.txn.lock();
        self.txn_with_origin_under_lock(origin)
    }

    /// [`Self::txn_with_origin`] for a caller that holds the doc's transaction lock.
    pub(crate) fn txn_under_lock(&self) -> Result<Transaction, LoroError> {
        self.txn_with_origin_under_lock("")
    }

    fn txn_with_origin_under_lock(&self, origin: &str) -> Result<Transaction, LoroError> {
        if !self.can_edit() {
            return Err(LoroError::TransactionError(
                String::from("LoroDoc is in readonly detached mode. To make it writable in detached mode, call `set_detached_editing(true)`.").into_boxed_str(),
            ));
        }

        let mut txn = Transaction::new_with_origin(Arc::clone(&self.inner), origin.into())?;
        txn.emit_on_commit = true;

        if self.has_max_doc_bytes() {
            self.arm_budget_scope_on_txn(&mut txn);
        }

        Ok(txn)
    }

    #[inline]
    pub(crate) fn has_post_commit_work(&self, events: &[DocDiff], id_span: IdSpan) -> bool {
        !events.is_empty()
            || (id_span.atom_len() != 0 && !self.local_update_subs.inner().is_empty_fast())
    }

    pub(crate) fn emit_post_commit(&self, post_commit: PostCommit, id_span: IdSpan) {
        // A panicking subscriber must not drop the other events, nor the
        // local update of this commit, which apps send to their peers.
        let mut panic = DeferredPanic::default();
        for event in post_commit.events {
            panic.run(|| self.observer.emit(event));
        }

        if id_span.atom_len() != 0 && !self.local_update_subs.inner().is_empty_fast() {
            let bytes = post_commit
                .local_update
                .unwrap_or_else(|| export_fast_updates_in_range(&self.oplog.lock(), &[id_span]));
            panic.run(|| self.local_update_subs.emit(&(), bytes));
        }
        panic.resume();
    }

    pub fn start_auto_commit(&self) {
        self.auto_commit
            .store(true, std::sync::atomic::Ordering::Release);
        let mut self_txn = self.txn.lock();
        if self_txn.is_some() || !self.can_edit() {
            return;
        }

        *self_txn = self.auto_commit_txn(None);
    }

    /// [`Self::start_auto_commit`] for a caller about to write: it returns why the
    /// transaction cannot start, see [`Self::auto_commit_txn`].
    pub(crate) fn try_start_auto_commit(&self) -> LoroResult<()> {
        self.auto_commit
            .store(true, std::sync::atomic::Ordering::Release);
        let mut self_txn = self.txn.lock();
        if self_txn.is_none() && self.can_edit() {
            *self_txn = Some(self.txn_under_lock()?);
        }

        Ok(())
    }

    /// The next auto-commit transaction, or `None` when the peer cannot write here.
    ///
    /// In detached editing, a custom peer id that already has ops after the state
    /// version cannot start a transaction. That is no fault of the commit, export
    /// or import that renews it, so they go on with no pending transaction. The
    /// next write tries again in `with_txn` and reports
    /// [`LoroError::ConcurrentOpsWithSamePeerID`] to its caller.
    ///
    /// The same goes for a manual transaction that is still open: the next write
    /// reports [`LoroError::DuplicatedTransactionError`].
    pub(crate) fn auto_commit_txn(&self, options: Option<CommitOptions>) -> Option<Transaction> {
        match self.txn_under_lock() {
            Ok(mut txn) => {
                if let Some(options) = options {
                    txn.set_options(options);
                }
                Some(txn)
            }
            Err(
                LoroError::ConcurrentOpsWithSamePeerID { .. }
                | LoroError::DuplicatedTransactionError,
            ) => None,
            Err(err) => panic!("auto-commit should be able to create a transaction: {err}"),
        }
    }

    #[inline]
    pub fn renew_txn_if_auto_commit(&self, options: Option<CommitOptions>) {
        if self.auto_commit.load(std::sync::atomic::Ordering::Acquire) && self.can_edit() {
            let mut self_txn = self.txn.lock();
            if self_txn.is_some() {
                return;
            }

            *self_txn = self.auto_commit_txn(options);
        }
    }

    #[inline]
    pub(crate) fn _renew_txn_if_auto_commit_with_guard(
        &self,
        options: Option<CommitOptions>,
        mut guard: LoroMutexGuard<Option<Transaction>>,
    ) {
        if self.auto_commit.load(std::sync::atomic::Ordering::Acquire) && self.can_edit() {
            if guard.is_some() {
                return;
            }

            *guard = self.auto_commit_txn(options);
        }
    }
}

pub struct Transaction {
    peer: PeerID,
    origin: InternalString,
    start_counter: Counter,
    next_counter: Counter,
    start_lamport: Lamport,
    next_lamport: Lamport,
    doc: Weak<LoroDocInner>,
    frontiers: Frontiers,
    local_ops: RleVec<[Op; 1]>, // TODO: use a more efficient data structure
    /// Sum of [`Op::payload_heap_bytes`] over `local_ops`, kept as ops are pushed
    /// and rolled back so budget checks never walk the transaction.
    local_ops_payload_bytes: usize,
    event_hints: FxHashMap<ContainerIdx, Vec<EventHint>>,
    pub(super) arena: SharedArena,
    finished: bool,
    emit_on_commit: bool,
    timestamp: Option<Timestamp>,
    msg: Option<Arc<str>>,
    latest_timestamp: Timestamp,
    pub(super) is_peer_first_appearance: bool,
    /// The DAG's frontiers when the txn began, before its first op.
    dag_frontiers_at_start: Frontiers,
    /// Active nested rollback scopes on this transaction (`budget_base_scope`
    /// and any inner `apply_diff` / `revert_to` / `undo` batch).
    rollback_scopes: Vec<RollbackScope>,
    /// Outermost rollback scope armed when `MemoryBudget::max_doc_bytes` or a global hard limit is set.
    budget_base_scope: Option<u64>,
    /// Hard quota captured before this transaction accepts edits.
    pub(crate) doc_budget_limit: Option<usize>,
    /// WASM linear memory bytes captured when `budget_base_scope` was armed (`0` on native).
    budget_scope_linear_bytes: u64,
    /// The inverse of each op, for undo managers; see [`LocalInverseRecorder`].
    local_inverse: LocalInverseRecorder,
}

#[derive(Debug)]
enum RecordedOp {
    CompactInsert {
        container: ContainerIdx,
        pos: u32,
        len: u32,
    },
    /// The event-level diff that undoes the op.
    Diff {
        container: ContainerIdx,
        diff: Diff,
        /// For a map op, its key and the value it wrote.
        map_written: Option<(InternalString, Option<LoroValue>)>,
    },
    /// A movable-list op: what its identity undo plan reads. Its positional
    /// inverse is built from it when used.
    Movable(crate::undo::movable_list::RecordedInverse),
    /// A counter increment, kept as is: a checkout sums a run's increments in
    /// op order, and float addition is not associative.
    #[cfg(feature = "counter")]
    Counter {
        container: ContainerIdx,
        increment: f64,
    },
}

impl RecordedOp {
    fn try_merge(&mut self, next: &Self) -> bool {
        let (
            Self::CompactInsert {
                container,
                pos,
                len,
            },
            Self::CompactInsert {
                container: next_container,
                pos: next_pos,
                len: next_len,
            },
        ) = (self, next)
        else {
            return false;
        };
        if container != next_container || *next_pos < *pos || *next_pos > pos.saturating_add(*len) {
            return false;
        }
        let Some(merged_len) = len.checked_add(*next_len) else {
            return false;
        };
        *len = merged_len;
        true
    }

    /// What the record holds, for the transaction's memory estimate.
    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + match self {
                RecordedOp::CompactInsert { .. } => 0,
                RecordedOp::Diff {
                    diff, map_written, ..
                } => {
                    diff.estimated_bytes()
                        + map_written.as_ref().map_or(0, |(key, value)| {
                            key.len()
                                + value.as_ref().map_or(0, |v| {
                                    v.estimated_heap_bytes()
                                        .saturating_sub(std::mem::size_of::<LoroValue>())
                                })
                        })
                }
                RecordedOp::Movable(record) => record.estimated_bytes(),
                #[cfg(feature = "counter")]
                RecordedOp::Counter { .. } => 0,
            }
    }
}

/// Records the inverse of every local op of a transaction while an undo manager
/// is attached, so undo does not have to check the document out to get it
/// (context/undo-log.md). Each entry is the event-level diff that undoes one op,
/// in the coordinates right after it.
#[derive(Debug, Default)]
enum LocalInverseRecorder {
    /// Not decided yet (decided at the first op).
    #[default]
    Unknown,
    /// No undo manager; or an op whose inverse is not recorded (or a rolled
    /// back batch), so undo checks the document out for this commit.
    Off,
    /// Undo manager active; waiting for the first recorded op.
    Empty,
    /// Fast inline state for a single (or coalesced) compact Text/List insertion
    /// without allocating a `Vec<RecordedOp>`.
    SingleCompact {
        container: ContainerIdx,
        pos: u32,
        len: u32,
    },
    /// Each op's inverse; see [`RecordedOp`]. `bytes`: their estimated size,
    /// counted in the transaction's uncommitted bytes.
    Recording { ops: Vec<RecordedOp>, bytes: usize },
}

impl LocalInverseRecorder {
    #[inline]
    fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Empty | Self::SingleCompact { .. } | Self::Recording { .. }
        )
    }

    fn push(&mut self, op: RecordedOp) {
        match self {
            Self::Empty => {
                if let RecordedOp::CompactInsert {
                    container,
                    pos,
                    len,
                } = op
                {
                    *self = Self::SingleCompact {
                        container,
                        pos,
                        len,
                    };
                } else {
                    let bytes = op.estimated_bytes();
                    *self = Self::Recording {
                        ops: vec![op],
                        bytes,
                    };
                }
            }
            Self::SingleCompact {
                container,
                pos,
                len,
            } => {
                if let RecordedOp::CompactInsert {
                    container: next_container,
                    pos: next_pos,
                    len: next_len,
                } = &op
                {
                    if *container == *next_container
                        && *next_pos >= *pos
                        && *next_pos <= pos.saturating_add(*len)
                    {
                        if let Some(merged_len) = len.checked_add(*next_len) {
                            *len = merged_len;
                            return;
                        }
                    }
                }
                let first = RecordedOp::CompactInsert {
                    container: *container,
                    pos: *pos,
                    len: *len,
                };
                let bytes = first.estimated_bytes() + op.estimated_bytes();
                *self = Self::Recording {
                    ops: vec![first, op],
                    bytes,
                };
            }
            Self::Recording { ops, bytes } => {
                if !ops.last_mut().is_some_and(|last| last.try_merge(&op)) {
                    *bytes += op.estimated_bytes();
                    ops.push(op);
                }
            }
            Self::Unknown | Self::Off => {}
        }
    }
}

/// See [`Transaction::begin_rollback_scope`].
#[derive(Debug)]
struct RollbackScope {
    id: u64,
    thread: usize,
    ignore_thread: bool,
    /// Another thread applied an op in this txn while the batch ran.
    foreign_ops: bool,
    /// The txn's next counter and lamport when the batch began.
    counter: Counter,
    lamport: Lamport,
    is_peer_first_appearance: bool,
    /// For each container the batch gave an event hint to: how many hints it
    /// had before, and the last one, which the batch's first hint may have been
    /// merged into.
    event_hints: FxHashMap<ContainerIdx, (usize, Option<EventHint>)>,
}

/// Identifies the current thread for [`RollbackScope`]: the address of a
/// thread local is unique among the live threads.
fn current_thread_mark() -> usize {
    thread_local!(static MARK: u8 = const { 0 });
    MARK.with(|m| m as *const u8 as usize)
}

/// What [`Transaction::roll_back_scope`] took out of a txn.
pub(crate) struct RolledBackBatch {
    pub(crate) peer: PeerID,
    /// The txn's first op, its lamport, and the version it was applied on.
    pub(crate) txn_start_counter: Counter,
    pub(crate) txn_start_lamport: Lamport,
    pub(crate) deps: Frontiers,
    /// The DAG's frontiers before the txn's first op.
    pub(crate) dag_frontiers_at_start: Frontiers,
    /// The batch's ops are `batch_start..batch_end`; the txn keeps the ops before.
    pub(crate) batch_start: Counter,
    pub(crate) batch_end: Counter,
    /// Every op of the txn, including the batch's.
    pub(crate) ops: RleVec<[Op; 1]>,
    pub(crate) timestamp: Timestamp,
}

impl std::fmt::Debug for Transaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Transaction")
            .field("peer", &self.peer)
            .field("origin", &self.origin)
            .field("start_counter", &self.start_counter)
            .field("next_counter", &self.next_counter)
            .field("start_lamport", &self.start_lamport)
            .field("next_lamport", &self.next_lamport)
            .field("frontiers", &self.frontiers)
            .field("local_ops", &self.local_ops)
            .field("event_hints", &self.event_hints)
            .field("arena", &self.arena)
            .field("finished", &self.finished)
            .field("emit_on_commit", &self.emit_on_commit)
            .field("timestamp", &self.timestamp)
            .finish()
    }
}

/// We can infer local events directly from the local behavior. This enum is used to
/// record them, so that we can avoid recalculate them when we commit the transaction.
///
/// For example, when we insert a text in wasm, users use the utf16 index to send the
/// command. However, internally loro will convert it to unicode index. But the users
/// still need events that are in utf16 index. To avoid the round trip, we record the
/// events here.
#[derive(Debug, Clone, EnumAsInner)]
pub(super) enum EventHint {
    Mark {
        start: u32,
        end: u32,
        style: Style,
    },
    InsertText {
        /// pos is a Unicode index. If wasm, it's a UTF-16 index.
        pos: u32,
        event_len: u32,
        unicode_len: u32,
        styles: StyleMeta,
    },
    /// pos is a Unicode index. If wasm, it's a UTF-16 index.
    DeleteText {
        span: DeleteSpan,
        unicode_len: usize,
    },
    InsertList {
        len: u32,
        pos: usize,
    },
    SetList {
        index: usize,
        value: LoroValue,
    },
    Move {
        value: LoroValue,
        from: u32,
        to: u32,
    },
    DeleteList(DeleteSpan),
    Map {
        key: InternalString,
        value: Option<LoroValue>,
    },
    // use vec because we could bring back some node that has children
    Tree(SmallVec<[TreeDiffItem; 1]>),
    /// A deleted node moved back by `apply_diff` (undo, redo, `revert_to`): `Create`s for
    /// the node and its subtree, then the full state of each metadata map and the
    /// containers it holds, read when the op was made. Remote peers get the same from
    /// `trigger_on_new_container`; see context/tree-events.md.
    TreeRevive {
        items: Vec<TreeDiffItem>,
        contents: Vec<(ContainerIdx, Diff)>,
    },
    MarkEnd,
    #[cfg(feature = "counter")]
    Counter(f64),
}

impl generic_btree::rle::HasLength for EventHint {
    fn rle_len(&self) -> usize {
        match self {
            EventHint::Mark { .. } => 1,
            EventHint::InsertText {
                unicode_len: len, ..
            } => *len as usize,
            EventHint::DeleteText { unicode_len, .. } => *unicode_len,
            EventHint::InsertList { len, .. } => *len as usize,
            EventHint::DeleteList(d) => d.len(),
            EventHint::Map { .. } => 1,
            EventHint::Tree(_) => 1,
            EventHint::TreeRevive { .. } => 1,
            EventHint::MarkEnd => 1,
            EventHint::Move { .. } => 1,
            EventHint::SetList { .. } => 1,
            #[cfg(feature = "counter")]
            EventHint::Counter(_) => 1,
        }
    }
}

impl generic_btree::rle::Mergeable for EventHint {
    fn can_merge(&self, rhs: &Self) -> bool {
        match (self, rhs) {
            (
                EventHint::InsertText {
                    pos,
                    unicode_len: _,
                    event_len,
                    styles,
                },
                EventHint::InsertText {
                    pos: r_pos,
                    styles: r_styles,
                    ..
                },
            ) => *pos + *event_len == *r_pos && styles == r_styles,
            (EventHint::InsertList { pos, len }, EventHint::InsertList { pos: pos_right, .. }) => {
                pos + *len as usize == *pos_right
            }
            // We don't merge delete text because it's hard to infer the correct pos to split:
            // `range` param is in unicode range, but the delete text event is in UTF-16 range.
            // Without the original text, it's impossible to convert the range.
            (EventHint::DeleteText { span, .. }, EventHint::DeleteText { span: r, .. }) => {
                span.is_mergable(r, &())
            }
            (EventHint::DeleteList(l), EventHint::DeleteList(r)) => l.is_mergable(r, &()),
            _ => false,
        }
    }

    fn merge_right(&mut self, rhs: &Self) {
        match (self, rhs) {
            (
                EventHint::InsertText {
                    event_len,
                    unicode_len: len,
                    ..
                },
                EventHint::InsertText {
                    event_len: r_event_len,
                    unicode_len: r_len,
                    ..
                },
            ) => {
                *len += *r_len;
                *event_len += *r_event_len;
            }
            (
                EventHint::InsertList { len, pos: _ },
                EventHint::InsertList { len: r_len, pos: _ },
            ) => *len += *r_len,
            (EventHint::DeleteList(l), EventHint::DeleteList(r)) => l.merge(r, &()),
            (
                EventHint::DeleteText { span, unicode_len },
                EventHint::DeleteText {
                    span: r_span,
                    unicode_len: r_len,
                },
            ) => {
                *unicode_len += *r_len;
                span.merge(r_span, &());
            }
            _ => unreachable!(),
        }
    }

    fn merge_left(&mut self, _: &Self) {
        unreachable!()
    }
}

impl Transaction {
    #[inline]
    pub fn new(doc: Arc<LoroDocInner>) -> LoroResult<Self> {
        Self::new_with_origin(doc.clone(), "".into())
    }

    pub fn new_with_origin(doc: Arc<LoroDocInner>, origin: InternalString) -> LoroResult<Self> {
        let oplog_lock = doc.oplog.lock();
        let mut state_lock = doc.state.lock();
        if state_lock.is_in_txn() {
            return Err(LoroError::DuplicatedTransactionError);
        }

        state_lock.start_txn(origin, crate::event::EventTriggerKind::Local);
        let arena = state_lock.arena.clone();
        let frontiers = state_lock.frontiers.clone();
        let peer = state_lock.peer.load(std::sync::atomic::Ordering::Relaxed);
        let next_counter = oplog_lock.next_id(peer).counter;
        let next_lamport = oplog_lock.dag.frontiers_to_next_lamport(&frontiers);
        let dag_frontiers_at_start = oplog_lock.dag.frontiers().clone();
        let latest_timestamp = oplog_lock.get_greatest_timestamp(&frontiers);
        if let Err(err) =
            oplog_lock.check_change_greater_than_last_peer_id(peer, next_counter, &frontiers)
        {
            state_lock.abort_txn();
            return Err(err);
        }
        drop(state_lock);
        drop(oplog_lock);
        Ok(Self {
            peer,
            doc: Arc::downgrade(&doc),
            arena,
            frontiers,
            timestamp: None,
            next_counter,
            next_lamport,
            origin: Default::default(),
            start_counter: next_counter,
            start_lamport: next_lamport,
            event_hints: Default::default(),
            local_ops: RleVec::new(),
            local_ops_payload_bytes: 0,
            finished: false,
            emit_on_commit: false,
            msg: None,
            latest_timestamp,
            is_peer_first_appearance: false,
            dag_frontiers_at_start,
            rollback_scopes: Vec::new(),
            budget_base_scope: None,
            doc_budget_limit: None,
            budget_scope_linear_bytes: crate::memory::wasm_linear_memory_bytes(),
            local_inverse: LocalInverseRecorder::Unknown,
        })
    }

    pub fn set_origin(&mut self, origin: InternalString) {
        self.origin = origin;
    }

    pub fn set_timestamp(&mut self, time: Timestamp) {
        self.timestamp = Some(time);
    }

    pub fn set_msg(&mut self, msg: Option<Arc<str>>) {
        self.msg = msg;
    }

    pub fn local_ops(&self) -> &RleVec<[Op; 1]> {
        &self.local_ops
    }

    pub fn peer(&self) -> &PeerID {
        &self.peer
    }

    pub fn timestamp(&self) -> &Option<Timestamp> {
        &self.timestamp
    }

    pub fn frontiers(&self) -> &Frontiers {
        &self.frontiers
    }
    pub fn msg(&self) -> &Option<Arc<str>> {
        &self.msg
    }

    pub fn lamport(&self) -> &Lamport {
        &self.start_lamport
    }

    pub(crate) fn take_emit_on_commit(&mut self) -> bool {
        std::mem::replace(&mut self.emit_on_commit, false)
    }

    pub fn commit(mut self) -> Result<Option<CommitOptions>, LoroError> {
        if !self.finished
            && !self.local_ops.is_empty()
            && (self.doc_budget_limit.is_some() || self.budget_base_scope.is_some())
        {
            if let Some(doc_inner) = self.doc.upgrade() {
                let doc = LoroDoc::borrow_from_inner(&doc_inner);
                if self.doc_budget_limit.is_some()
                    || (doc.has_max_doc_bytes() && self.budget_base_scope.is_some())
                {
                    let _txn_guard = doc.txn.lock();
                    let uncommitted = self.estimated_uncommitted_bytes();
                    doc.enforce_memory_watermarks_with_txn_bytes(uncommitted, false);
                    if let Err(budget_err) =
                        doc.check_transaction_budget(&self, Some(self.budget_scope_linear_bytes))
                    {
                        if let Some(scope) = self.budget_base_scope.take() {
                            self.finished = true;
                            doc.rollback_transaction_scope(&mut self, scope)?;
                            let mut state = doc.state.lock();
                            state.abort_txn();
                            state.clear_local_rollback_stack();
                            drop(state);
                            doc.cleanup_rollback_transient_caches(&budget_err);
                            doc.refresh_cached_doc_bytes_unlocked();
                            return Err(budget_err);
                        }
                    }
                }
            }
        }
        let mut panic = DeferredPanic::default();
        let ans = self._commit(&mut panic, false).map(|(opts, _)| opts);
        panic.resume();
        ans
    }

    /// [`Self::commit`] for a caller that holds the doc's txn lock: a panic of a
    /// pre-commit callback is put in `panic`, to be raised after the lock is
    /// released.
    pub(crate) fn commit_deferring_panic(
        mut self,
        panic: &mut DeferredPanic,
        take_events_for_caller: bool,
    ) -> Result<(Option<CommitOptions>, PostCommit), LoroError> {
        self._commit(panic, take_events_for_caller)
    }

    /// A pre-commit callback that panics is user code panicking in the middle
    /// of a commit: the change is already applied to the state and the op log
    /// holds it as uncommitted. The commit is finished as if the callback had
    /// returned, and its panic is put in `panic` to be raised once no doc lock
    /// is held.
    #[tracing::instrument(level = "debug", skip(self, panic))]
    fn _commit(
        &mut self,
        panic: &mut DeferredPanic,
        take_events_for_caller: bool,
    ) -> Result<(Option<CommitOptions>, PostCommit), LoroError> {
        if self.finished {
            return Ok((None, PostCommit::default()));
        }

        let Some(doc) = self.doc.upgrade() else {
            return Ok((None, PostCommit::default()));
        };
        self.finished = true;
        self.rollback_scopes.clear();
        self.budget_base_scope = None;
        let emit_here = std::mem::replace(&mut self.emit_on_commit, false);
        if self.local_ops.is_empty() {
            let mut state = doc.state.lock();
            state.abort_txn();
            state.clear_local_rollback_stack();
            let events = if take_events_for_caller || emit_here {
                state.take_events()
            } else {
                Vec::new()
            };
            drop(state);
            if emit_here {
                let doc_ref = LoroDoc::borrow_from_inner(&doc);
                if doc_ref.has_post_commit_work(&events, self.id_span()) {
                    debug_assert!(doc.txn.can_lock_in_this_thread());
                    doc_ref.emit_post_commit(
                        PostCommit {
                            events,
                            local_update: None,
                        },
                        self.id_span(),
                    );
                }
                return Ok((Some(self.take_options()), PostCommit::default()));
            }
            return Ok((
                Some(self.take_options()),
                PostCommit {
                    events,
                    local_update: None,
                },
            ));
        }

        let ops = std::mem::take(&mut self.local_ops);
        self.local_ops_payload_bytes = 0;
        let deps = take(&mut self.frontiers);
        let has_pre_commit = !doc.pre_commit_subs.inner().is_empty_fast();

        let (mut oplog, mut state, change) = if has_pre_commit {
            let change = Change {
                lamport: self.start_lamport,
                ops,
                deps,
                id: ID::new(self.peer, self.start_counter),
                timestamp: self.latest_timestamp.max(
                    self.timestamp
                        .unwrap_or_else(|| doc.oplog.lock().get_timestamp_for_next_txn()),
                ),
                commit_msg: take(&mut self.msg),
            };
            let change_meta = ChangeMeta::from_change(&change);
            {
                // add change to uncommit field of oplog
                let mut oplog = doc.oplog.lock();
                oplog.set_uncommitted_change(change);
            }

            let modifier = ChangeModifier::default();
            panic.run(|| {
                doc.pre_commit_subs.emit(
                    &(),
                    PreCommitCallbackPayload {
                        change_meta,
                        origin: self.origin.to_string(),
                        modifier: modifier.clone(),
                    },
                )
            });

            let mut oplog = doc.oplog.lock();
            let mut state = doc.state.lock();

            let Some(mut change) = oplog.uncommitted_change.take() else {
                state.abort_txn();
                drop(state);
                drop(oplog);
                return Err(LoroError::internal(
                    "missing uncommitted change while committing transaction",
                ));
            };
            modifier.modify_change(&mut change);
            (oplog, state, change)
        } else {
            let oplog = doc.oplog.lock();
            let state = doc.state.lock();
            let timestamp = self.latest_timestamp.max(
                self.timestamp
                    .unwrap_or_else(|| oplog.get_timestamp_for_next_txn()),
            );
            let change = Change {
                lamport: self.start_lamport,
                ops,
                deps,
                id: ID::new(self.peer, self.start_counter),
                timestamp,
                commit_msg: take(&mut self.msg),
            };
            (oplog, state, change)
        };

        let diff = if state.is_recording() {
            Some(change_to_diff(
                &change,
                doc.clone(),
                std::mem::take(&mut self.event_hints),
            ))
        } else {
            None
        };

        let last_id = change.id_last();
        let change_deps = self.local_inverse.is_active().then(|| change.deps.clone());
        let encode_update =
            (emit_here || take_events_for_caller) && !doc.local_update_subs.inner().is_empty_fast();
        let local_update = match oplog.import_local_change(change, encode_update) {
            Ok(update) => update,
            Err(err) => {
                state.abort_txn();
                state.clear_local_rollback_stack();
                drop(state);
                drop(oplog);
                return Err(err);
            }
        };

        let local_inverse = match std::mem::take(&mut self.local_inverse) {
            LocalInverseRecorder::SingleCompact {
                container,
                pos,
                len,
            } if diff.is_some() => build_compact_local_inverse(
                container,
                pos,
                len,
                self.id_span(),
                change_deps.unwrap(),
                &state.arena,
            )
            .map(Arc::new),
            LocalInverseRecorder::Recording { ops, .. } if diff.is_some() => {
                build_local_inverse(ops, self.id_span(), change_deps.unwrap(), &mut state)
                    .map(Arc::new)
            }
            _ => None,
        };
        state.commit_txn(
            Frontiers::from_id(last_id),
            diff.map(|arr| InternalDocDiff {
                local_inverse,
                by: crate::event::EventTriggerKind::Local,
                origin: self.origin.clone(),
                diff: Cow::Owned(
                    arr.into_iter()
                        .map(|x| InternalContainerDiff {
                            idx: x.idx,
                            bring_back: false,
                            diff: (x.diff.into()),
                            diff_mode: crate::diff_calc::DiffMode::Linear,
                            kept: None,
                        })
                        .collect(),
                ),
                new_version: Cow::Borrowed(oplog.frontiers()),
            }),
        );
        let events = if take_events_for_caller || emit_here {
            state.take_events()
        } else {
            Vec::new()
        };
        drop(state);
        drop(oplog);
        let doc_ref = LoroDoc::borrow_from_inner(&doc);
        if emit_here {
            if doc_ref.has_post_commit_work(&events, self.id_span()) {
                // Not under this thread's transaction lock. Another thread may hold it: a
                // manual transaction commits without it.
                debug_assert!(doc.txn.can_lock_in_this_thread());
                doc_ref.emit_post_commit(
                    PostCommit {
                        events,
                        local_update,
                    },
                    self.id_span(),
                );
            }
            doc_ref.on_commit_completed();
            return Ok((None, PostCommit::default()));
        }
        doc_ref.on_commit_completed();
        Ok((
            None,
            PostCommit {
                events,
                local_update,
            },
        ))
    }

    pub(crate) fn estimated_uncommitted_bytes(&self) -> usize {
        let ops_bytes = (self.local_ops.len() * std::mem::size_of::<Op>())
            .saturating_add(self.local_ops_payload_bytes);
        let hints_bytes = self.event_hints.capacity()
            * (std::mem::size_of::<ContainerIdx>() + std::mem::size_of::<Vec<EventHint>>() + 8)
            + self
                .event_hints
                .values()
                .map(|v| v.capacity() * std::mem::size_of::<EventHint>())
                .sum::<usize>();
        let inverse_bytes = match &self.local_inverse {
            LocalInverseRecorder::SingleCompact { .. } => std::mem::size_of::<RecordedOp>(),
            LocalInverseRecorder::Recording { ops, bytes } => {
                let spare = ops.capacity().saturating_sub(ops.len());
                bytes.saturating_add(spare.saturating_mul(std::mem::size_of::<RecordedOp>()))
            }
            _ => 0,
        };
        ops_bytes
            .saturating_add(hints_bytes)
            .saturating_add(inverse_bytes)
    }

    pub(crate) fn budget_base_scope(&self) -> Option<u64> {
        self.budget_base_scope
    }

    pub(crate) fn budget_scope_linear_bytes(&self) -> u64 {
        self.budget_scope_linear_bytes
    }

    /// Mark the start of a batch that `LoroDoc` rolls back if it fails
    /// ([`Self::roll_back_scope`]). The txn may already hold ops; they are kept.
    pub(crate) fn begin_rollback_scope(&mut self, id: u64) {
        self.begin_rollback_scope_inner(id, false);
    }

    pub(crate) fn begin_budget_rollback_scope(&mut self, id: u64) {
        debug_assert!(self.rollback_scopes.is_empty());
        self.budget_base_scope = Some(id);
        self.budget_scope_linear_bytes = crate::memory::wasm_linear_memory_bytes();
        self.begin_rollback_scope_inner(id, true);
    }

    fn begin_rollback_scope_inner(&mut self, id: u64, ignore_thread: bool) {
        self.rollback_scopes.push(RollbackScope {
            id,
            thread: current_thread_mark(),
            ignore_thread,
            foreign_ops: false,
            counter: self.next_counter,
            lamport: self.next_lamport,
            is_peer_first_appearance: self.is_peer_first_appearance,
            event_hints: FxHashMap::default(),
        });
    }

    pub(crate) fn has_rollback_scope(&self) -> bool {
        self.rollback_scopes
            .iter()
            .any(|s| Some(s.id) != self.budget_base_scope)
    }

    pub(crate) fn end_rollback_scope(&mut self, id: u64) {
        if self.rollback_scopes.last().is_some_and(|s| s.id == id) {
            self.rollback_scopes.pop();
            if self.budget_base_scope == Some(id) {
                self.budget_base_scope = None;
            }
        }
    }

    /// Whether `id` is currently the innermost txn scope and every op in it came
    /// from the thread that began it (unless `ignore_thread` is set).
    pub(crate) fn rollback_scope_is_intact(&self, id: u64) -> bool {
        self.rollback_scopes
            .last()
            .is_some_and(|s| s.id == id && !s.foreign_ops)
    }

    /// Take the ops of scope `id` (which must be intact) out of the txn: it
    /// keeps its ops from before the scope, and its counters and event hints are
    /// as they were then. The ops stay applied to the state and the DAG's
    /// version; the caller undoes them.
    pub(crate) fn roll_back_scope(&mut self, id: u64) -> RolledBackBatch {
        assert!(self.rollback_scope_is_intact(id));
        if self.local_inverse.is_active() {
            self.local_inverse = LocalInverseRecorder::Off;
        }
        let scope = self.rollback_scopes.pop().unwrap();
        if self.budget_base_scope == Some(id) {
            self.budget_base_scope = None;
        }
        let ops = take(&mut self.local_ops);
        self.local_ops_payload_bytes = 0;
        for op in ops.iter() {
            let end = op.counter + op.atom_len() as Counter;
            let kept = if end <= scope.counter {
                op.clone()
            } else if op.counter < scope.counter {
                op.slice(0, (scope.counter - op.counter) as usize)
            } else {
                continue;
            };
            self.local_ops_payload_bytes = self
                .local_ops_payload_bytes
                .saturating_add(kept.payload_heap_bytes());
            self.local_ops.push(kept);
        }
        for (container, (len, last)) in scope.event_hints {
            if len == 0 {
                self.event_hints.remove(&container);
                continue;
            }
            let hints = self.event_hints.get_mut(&container).unwrap();
            hints.truncate(len);
            *hints.last_mut().unwrap() = last.unwrap();
        }
        if self.event_hints.is_empty() {
            self.event_hints = FxHashMap::default();
        } else if self.event_hints.capacity() > self.event_hints.len().saturating_mul(2).max(4) {
            self.event_hints.shrink_to_fit();
        }
        let batch_end = self.next_counter;
        self.next_counter = scope.counter;
        self.next_lamport = scope.lamport;
        self.is_peer_first_appearance = scope.is_peer_first_appearance;
        RolledBackBatch {
            peer: self.peer,
            txn_start_counter: self.start_counter,
            txn_start_lamport: self.start_lamport,
            deps: self.frontiers.clone(),
            dag_frontiers_at_start: self.dag_frontiers_at_start.clone(),
            batch_start: scope.counter,
            batch_end,
            ops,
            timestamp: self.latest_timestamp,
        }
    }

    fn take_options(&self) -> CommitOptions {
        let mut options = CommitOptions::new();
        if !self.origin.is_empty() {
            options = options.origin(self.origin.as_str());
        }
        if let Some(msg) = self.msg.as_ref() {
            options = options.commit_msg(msg);
        }
        if let Some(timestamp) = self.timestamp {
            options = options.timestamp(timestamp);
        }
        options
    }

    pub(super) fn apply_local_op(
        &mut self,
        container: ContainerIdx,
        content: RawOpContent,
        event: EventHint,
        doc: &LoroDoc,
    ) -> LoroResult<()> {
        self.apply_local_op_inner(container, content, event, doc, false)
    }

    pub(super) fn apply_local_map_insert(
        &mut self,
        container: ContainerIdx,
        content: RawOpContent,
        event: EventHint,
        doc: &LoroDoc,
    ) -> LoroResult<()> {
        self.apply_local_op_inner(container, content, event, doc, true)
    }

    fn apply_local_op_inner(
        &mut self,
        container: ContainerIdx,
        content: RawOpContent,
        event: EventHint,
        // check whether context and txn are referring to the same state context
        doc: &LoroDoc,
        skip_unchanged_map: bool,
    ) -> LoroResult<()> {
        // A transaction must be applied to the document it was created for.
        // `insert_with_txn`/`delete_with_txn` are public API, so this guards
        // against misuse (passing one doc's txn to another doc's handler), which
        // would otherwise stamp the target doc's state/oplog with the wrong
        // peer+counter. Compare doc identity via the `Weak` pointer (no atomic
        // upgrade) on the hot path; only upgrade to fill in the error on the
        // rare mismatch.
        if self.doc.as_ptr() != Arc::as_ptr(&doc.inner) {
            let found = doc
                .state
                .lock()
                .peer
                .load(std::sync::atomic::Ordering::Relaxed);
            let expected = self
                .doc
                .upgrade()
                .map(|d| {
                    d.state
                        .lock()
                        .peer
                        .load(std::sync::atomic::Ordering::Relaxed)
                })
                .unwrap_or(found);
            return Err(LoroError::UnmatchedContext { expected, found });
        }

        for scope in &mut self.rollback_scopes {
            if !scope.ignore_thread && scope.thread != current_thread_mark() {
                scope.foreign_ops = true;
            }
        }
        if self.local_ops.is_empty() && self.budget_base_scope.is_some() {
            self.budget_scope_linear_bytes = crate::memory::wasm_linear_memory_bytes();
        }

        let len = content.content_len();
        assert!(len > 0);

        if matches!(self.local_inverse, LocalInverseRecorder::Unknown) {
            let has_undo = doc
                .active_undo_managers
                .load(std::sync::atomic::Ordering::Acquire)
                > 0;
            self.local_inverse = if has_undo {
                LocalInverseRecorder::Empty
            } else {
                LocalInverseRecorder::Off
            };
        }
        let mut oplog = doc.oplog.lock();
        let mut state = doc.state.lock();
        if state.is_deleted(container) {
            return Err(LoroError::ContainerDeleted {
                container: Box::new(state.arena.idx_to_id(container).unwrap()),
            });
        }
        if skip_unchanged_map {
            if let EventHint::Map {
                key,
                value: Some(new_value),
            } = &event
            {
                if let Some(current) = state.get_map_value_by_key(container, key.as_ref()) {
                    let translated = match state.arena.idx_to_id(container) {
                        Some(id) => loro_common::translate_mergeable_marker_value(
                            &id,
                            key.as_ref(),
                            current,
                        ),
                        None => current,
                    };
                    if &translated == new_value {
                        return Ok(());
                    }
                }
            }
        }
        let raw_op = RawOp {
            id: ID {
                peer: self.peer,
                counter: self.next_counter,
            },
            lamport: self.next_lamport,
            container,
            content,
        };

        let op = self.arena.convert_raw_op(&raw_op);
        let inverse = self
            .local_inverse
            .is_active()
            .then(|| local_op_inverse(&mut state, container, &raw_op, &event, &doc.inner));
        state.apply_local_op(&raw_op, &op)?;
        if let Some(inverse) = inverse {
            let inverse = match inverse {
                // An op on a list's elements does not change whether the list
                // itself is reachable.
                Some(RecordedOp::Movable(mut record)) => state
                    .container_state_mut(container)
                    .as_movable_list_state()
                    .and_then(|list| record.finish(list))
                    .map(|()| RecordedOp::Movable(record)),
                other => other,
            };
            match inverse {
                Some(op) if self.local_inverse.is_active() => {
                    self.local_inverse.push(op);
                }
                _ => self.local_inverse = LocalInverseRecorder::Off,
            }
        }
        {
            if !self.is_peer_first_appearance && !oplog.dag.latest_vv_contains_peer(self.peer) {
                self.is_peer_first_appearance = true;
            }
            // update version info
            let dep_id = Frontiers::from_id(ID::new(self.peer, self.next_counter - 1));
            let start_id = ID::new(self.peer, self.next_counter);
            self.next_counter += len as Counter;
            oplog.dag.update_version_on_new_local_op(
                if self.local_ops.is_empty() {
                    &self.frontiers
                } else {
                    &dep_id
                },
                start_id,
                self.next_lamport,
                len,
            );
            // Local ops are always visible; bump the cached count incrementally
            // instead of recomputing it from the version vectors every op.
            oplog.inc_visible_op_count(len);
            self.next_lamport += len as Lamport;
            // set frontiers to the last op id
            let last_id = start_id.inc(len as Counter - 1);
            state.frontiers = Frontiers::from_id(last_id);
        };
        drop(state);
        drop(oplog);
        debug_assert_eq!(
            event.rle_len(),
            op.atom_len(),
            "event:{:#?} \nop:{:#?}",
            &event,
            &op
        );

        let container_hints = self.event_hints.entry(container).or_default();
        for scope in &mut self.rollback_scopes {
            scope
                .event_hints
                .entry(container)
                .or_insert_with(|| (container_hints.len(), container_hints.last().cloned()));
        }

        match container_hints.last_mut() {
            Some(last) if last.can_merge(&event) => {
                last.merge_right(&event);
            }
            _ => {
                container_hints.push(event);
            }
        }
        self.local_ops_payload_bytes = self
            .local_ops_payload_bytes
            .saturating_add(op.payload_heap_bytes());
        self.local_ops.push(op);
        Ok(())
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_text<I: IntoContainerId>(&self, id: I) -> TextHandler {
        let id = id.into_container_id(&self.arena, ContainerType::Text);
        Handler::new_attached(id, LoroDoc::from_inner(self.doc.upgrade().unwrap()))
            .into_text()
            .unwrap()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_list<I: IntoContainerId>(&self, id: I) -> ListHandler {
        let id = id.into_container_id(&self.arena, ContainerType::List);
        Handler::new_attached(id, LoroDoc::from_inner(self.doc.upgrade().unwrap()))
            .into_list()
            .unwrap()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_map<I: IntoContainerId>(&self, id: I) -> MapHandler {
        let id = id.into_container_id(&self.arena, ContainerType::Map);
        Handler::new_attached(id, LoroDoc::from_inner(self.doc.upgrade().unwrap()))
            .into_map()
            .unwrap()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_tree<I: IntoContainerId>(&self, id: I) -> TreeHandler {
        let id = id.into_container_id(&self.arena, ContainerType::Tree);
        Handler::new_attached(id, LoroDoc::from_inner(self.doc.upgrade().unwrap()))
            .into_tree()
            .unwrap()
    }
    pub fn next_id(&self) -> ID {
        ID {
            peer: self.peer,
            counter: self.next_counter,
        }
    }

    #[inline]
    pub fn id_span(&self) -> IdSpan {
        IdSpan::new(self.peer, self.start_counter, self.next_counter)
    }

    pub fn next_idlp(&self) -> IdLp {
        IdLp {
            peer: self.peer,
            lamport: self.next_lamport,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.local_ops.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        (self.next_counter - self.start_counter) as usize
    }

    pub(crate) fn set_options(&mut self, options: CommitOptions) {
        self.origin = options.origin.unwrap_or_default();
        self.msg = options.commit_msg;
        self.timestamp = options.timestamp;
    }

    pub(crate) fn set_default_options(&mut self, default_options: crate::loro::CommitOptions) {
        if self.origin.is_empty() {
            self.origin = default_options.origin.unwrap_or_default();
        }
        if self.msg.is_none() {
            self.msg = default_options.commit_msg;
        }
        if self.timestamp.is_none() {
            self.timestamp = default_options.timestamp;
        }
    }
}

impl Drop for Transaction {
    #[tracing::instrument(level = "debug", skip(self))]
    fn drop(&mut self) {
        if !self.finished {
            // TODO: should we abort here or commit here?
            // what if commit fails?
            let mut panic = DeferredPanic::default();
            let ans = self._commit(&mut panic, false);
            // Raising it while already unwinding would abort the process.
            if !std::thread::panicking() {
                panic.resume();
            }
            ans.unwrap();
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TxnContainerDiff {
    pub(crate) idx: ContainerIdx,
    pub(crate) diff: Diff,
}

// PERF: could be compacter
fn change_to_diff(
    change: &Change,
    doc: Arc<LoroDocInner>,
    event_hints: FxHashMap<ContainerIdx, Vec<EventHint>>,
) -> Vec<TxnContainerDiff> {
    let mut ans: Vec<TxnContainerDiff> = Vec::with_capacity(change.ops.len());
    let peer = change.id.peer;
    let mut lamport = change.lamport;

    // Group ops by container first to match our new structure
    let mut ops_by_container: FxHashMap<ContainerIdx, Vec<Op>> = FxHashMap::default();
    for op in change.ops.iter() {
        ops_by_container
            .entry(op.container)
            .or_default()
            .push(op.clone());
    }

    // Process each container's hints and ops together
    for (container_idx, hints) in event_hints {
        let Some(container_ops) = ops_by_container.get_mut(&container_idx) else {
            continue;
        };

        let mut op_index = 0;
        let mut hint_iter = hints.into_iter();
        let mut current_hint = hint_iter.next();

        while op_index < container_ops.len() {
            let Some(hint) = current_hint.take() else {
                unreachable!("Missing hint for op");
            };

            // Collect the ops that belong to this hint. Ops and hints merge by
            // different rules (text delete hints use UTF-16 spans in WASM), so a
            // hint can cover several ops and an op can cover several hints.
            let mut ops_for_hint: SmallVec<[Op; 1]> = SmallVec::new();
            let mut total_len = 0;
            while total_len < hint.rle_len() {
                let Some(op) = container_ops.get_mut(op_index) else {
                    unreachable!("Op/hint length mismatch: {hint:?}");
                };
                let op_len = op.atom_len();
                let needed = hint.rle_len() - total_len;
                if op_len > needed {
                    ops_for_hint.push(op.slice(0, needed));
                    *op = op.slice(needed, op_len);
                    total_len += needed;
                } else {
                    total_len += op_len;
                    ops_for_hint.push(op.clone());
                    op_index += 1;
                }
            }

            debug_assert!(
                ops_for_hint.iter().all(|op| op_matches_hint(op, &hint)),
                "Op/hint kind mismatch: {hint:?} {ops_for_hint:?}"
            );

            // Move to next hint
            current_hint = hint_iter.next();

            // Generate diff based on hint type
            match hint {
                EventHint::Mark { start, end, style } => {
                    let mut meta = StyleMeta::default();
                    meta.insert(
                        style.key.clone(),
                        StyleMetaItem {
                            lamport,
                            peer: change.id.peer,
                            value: style.data,
                        },
                    );
                    let diff = DeltaRopeBuilder::new()
                        .retain(start as usize, Default::default())
                        .retain(
                            (end - start) as usize,
                            meta.to_option_map().unwrap_or_default().into(),
                        )
                        .build();
                    ans.push(TxnContainerDiff {
                        idx: container_idx,
                        diff: Diff::Text(diff),
                    });
                }
                EventHint::InsertText { styles, pos, .. } => {
                    let mut delta: TextDiff = DeltaRopeBuilder::new()
                        .retain(pos as usize, Default::default())
                        .build();
                    for op in ops_for_hint.iter() {
                        let InnerListOp::InsertText { slice, .. } = op.content.as_list().unwrap()
                        else {
                            unreachable!()
                        };

                        delta.push_insert(
                            slice.clone().into(),
                            styles.to_option_map().unwrap_or_default().into(),
                        );
                    }
                    ans.push(TxnContainerDiff {
                        idx: container_idx,
                        diff: Diff::Text(delta),
                    })
                }
                EventHint::DeleteText {
                    span,
                    unicode_len: _,
                    // we don't need to iter over ops here, because we already
                    // know what the events should be
                } => ans.push(TxnContainerDiff {
                    idx: container_idx,
                    diff: Diff::Text(
                        DeltaRopeBuilder::new()
                            .retain(span.start() as usize, Default::default())
                            .delete(span.len())
                            .build(),
                    ),
                }),
                EventHint::InsertList { pos, .. } => {
                    // We should use pos from event hint because index in op may
                    // be using op index for the MovableList
                    let mut index = pos;
                    for op in ops_for_hint.iter() {
                        let (range, _) = op.content.as_list().unwrap().as_insert().unwrap();
                        let values = range
                            .as_slice()
                            .iter()
                            .map(|v| ValueOrHandler::from_value(v.clone(), &doc));
                        let len = values.len();
                        ans.push(TxnContainerDiff {
                            idx: container_idx,
                            diff: Diff::List(
                                DeltaRopeBuilder::new()
                                    .retain(index, Default::default())
                                    .insert_many(values, Default::default())
                                    .build(),
                            ),
                        });
                        index += len;
                    }
                }
                EventHint::DeleteList(s) => {
                    ans.push(TxnContainerDiff {
                        idx: container_idx,
                        diff: Diff::List(
                            DeltaRopeBuilder::new()
                                .retain(s.start() as usize, Default::default())
                                .delete(s.len())
                                .build(),
                        ),
                    });
                }
                EventHint::Map { key, value } => {
                    // Mirror `MapState::apply_diff_and_convert`: a marker write surfaces to
                    // subscribers as the deterministic Container, not the raw binary value.
                    let parent_id = doc.arena.idx_to_id(container_idx);
                    let value = value.map(|v| match parent_id.as_ref() {
                        Some(pid) => {
                            loro_common::translate_mergeable_marker_value(pid, key.as_ref(), v)
                        }
                        None => v,
                    });
                    ans.push(TxnContainerDiff {
                        idx: container_idx,
                        diff: Diff::Map(ResolvedMapDelta::new().with_entry(
                            key,
                            ResolvedMapValue {
                                value: value.map(|v| ValueOrHandler::from_value(v, &doc)),
                                idlp: IdLp::new(peer, lamport),
                            },
                        )),
                    });
                }
                EventHint::Tree(tree_diff) => {
                    let mut diff = TreeDiff::default();
                    diff.diff.extend(tree_diff);
                    ans.push(TxnContainerDiff {
                        idx: container_idx,
                        diff: Diff::Tree(diff),
                    });
                }
                EventHint::TreeRevive { items, contents } => {
                    let mut diff = TreeDiff::default();
                    diff.diff.extend(items);
                    ans.push(TxnContainerDiff {
                        idx: container_idx,
                        diff: Diff::Tree(diff),
                    });
                    ans.extend(
                        contents
                            .into_iter()
                            .map(|(idx, diff)| TxnContainerDiff { idx, diff }),
                    );
                }
                EventHint::Move { from, to, value } => {
                    let mut a = DeltaRopeBuilder::new()
                        .retain(from as usize, Default::default())
                        .delete(1)
                        .build();
                    a.compose(
                        &DeltaRopeBuilder::new()
                            .retain(to as usize, Default::default())
                            .insert(
                                ArrayVec::from([ValueOrHandler::from_value(value, &doc)]),
                                ListDeltaMeta { from_move: true },
                            )
                            .build(),
                    );
                    ans.push(TxnContainerDiff {
                        idx: container_idx,
                        diff: Diff::List(a),
                    });
                }
                EventHint::SetList { index, value } => {
                    ans.push(TxnContainerDiff {
                        idx: container_idx,
                        diff: Diff::List(
                            DeltaRopeBuilder::new()
                                .retain(index, Default::default())
                                .delete(1)
                                .insert(
                                    ArrayVec::from([ValueOrHandler::from_value(value, &doc)]),
                                    Default::default(),
                                )
                                .build(),
                        ),
                    });
                }
                EventHint::MarkEnd => {
                    // do nothing
                }
                #[cfg(feature = "counter")]
                EventHint::Counter(diff) => {
                    ans.push(TxnContainerDiff {
                        idx: container_idx,
                        diff: Diff::Counter(diff),
                    });
                }
            }

            // Update lamport for this hint's operations
            lamport += ops_for_hint
                .iter()
                .map(|x| x.content_len() as Lamport)
                .sum::<Lamport>();
        }

        debug_assert!(
            current_hint.is_none(),
            "Unused event hint: {current_hint:?}"
        );
    }

    ans
}

/// Whether `op` is the kind of op `hint` was recorded for.
fn op_matches_hint(op: &Op, hint: &EventHint) -> bool {
    match &op.content {
        InnerContent::List(list_op) => matches!(
            (list_op, hint),
            (InnerListOp::StyleStart { .. }, EventHint::Mark { .. })
                | (InnerListOp::StyleEnd, EventHint::MarkEnd)
                | (InnerListOp::InsertText { .. }, EventHint::InsertText { .. })
                | (
                    InnerListOp::Delete(_),
                    EventHint::DeleteText { .. } | EventHint::DeleteList(_)
                )
                | (InnerListOp::Insert { .. }, EventHint::InsertList { .. })
                | (InnerListOp::Move { .. }, EventHint::Move { .. })
                | (InnerListOp::Set { .. }, EventHint::SetList { .. })
        ),
        InnerContent::Map(_) => matches!(hint, EventHint::Map { .. }),
        InnerContent::Tree(_) => {
            matches!(hint, EventHint::Tree(_) | EventHint::TreeRevive { .. })
        }
        #[cfg(feature = "counter")]
        InnerContent::Future(crate::op::FutureInnerContent::Counter(_)) => {
            matches!(hint, EventHint::Counter(_))
        }
        InnerContent::Future(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cursor::PosType, version::Frontiers};

    #[test]
    fn txn_creation_rolls_back_in_txn_after_peer_conflict() {
        let doc = LoroDoc::new();
        doc.set_detached_editing(true);
        doc.set_peer_id(7).unwrap();

        let text = doc.get_text("text");
        let mut txn = doc.txn().unwrap();
        text.insert_with_txn(&mut txn, 0, "a", PosType::Unicode)
            .unwrap();
        txn.commit().unwrap();

        doc.checkout(&Frontiers::default()).unwrap();
        doc.set_peer_id(7).unwrap();

        let err = doc
            .txn()
            .expect_err("stale detached frontiers should reject reusing the same peer");
        assert!(matches!(
            err,
            LoroError::ConcurrentOpsWithSamePeerID { peer: 7, .. }
        ));
        assert!(!doc.app_state().lock().is_in_txn());
    }
}

fn build_compact_local_inverse(
    container: ContainerIdx,
    pos: u32,
    len: u32,
    span: IdSpan,
    deps: Frontiers,
    arena: &SharedArena,
) -> Option<crate::undo::LocalInverse> {
    let cid = arena.idx_to_id(container)?;
    if cid.is_mergeable() {
        return None;
    }
    let compact_insert = (!crate::undo::created_in(&cid, span)).then_some((cid, pos, len));
    Some(crate::undo::LocalInverse::new(
        crate::undo::CommitInverse {
            span,
            diff: crate::undo::DiffBatch::default(),
            compact_insert,
            map_values_after: FxHashMap::default(),
            movable: Vec::new(),
            counter_ops: Vec::new(),
        },
        deps,
    ))
}

/// The [`crate::undo::LocalInverse`] of a committed transaction: the inverse of
/// its last op first, then of each earlier one.
fn build_local_inverse(
    ops: Vec<RecordedOp>,
    span: IdSpan,
    deps: Frontiers,
    state: &mut DocState,
) -> Option<crate::undo::LocalInverse> {
    let arena = state.arena.clone();
    if let [RecordedOp::CompactInsert {
        container,
        pos,
        len,
    }] = ops.as_slice()
    {
        return build_compact_local_inverse(*container, *pos, *len, span, deps, &arena);
    }
    let single_op = ops.len() == 1;
    let mut cids: FxHashMap<ContainerIdx, loro_common::ContainerID> = FxHashMap::default();
    let mut cid_of = |idx: ContainerIdx| {
        if single_op {
            return arena
                .idx_to_id(idx)
                .expect("an edited container is registered");
        }
        cids.entry(idx)
            .or_insert_with(|| {
                arena
                    .idx_to_id(idx)
                    .expect("an edited container is registered")
            })
            .clone()
    };
    let mut map_values_after = FxHashMap::default();
    let mut movable: Vec<crate::undo::movable_list::RecordedInverse> = Vec::new();
    #[allow(unused_mut)]
    let mut counter_ops: Vec<(loro_common::ContainerID, f64)> = Vec::new();
    let mut inverses =
        smallvec::SmallVec::<[(loro_common::ContainerID, Diff); 1]>::with_capacity(ops.len());
    for op in ops {
        match op {
            RecordedOp::CompactInsert {
                container,
                pos,
                len,
            } => {
                let cid = cid_of(container);
                let op_diff = match cid.container_type() {
                    ContainerType::Text => Diff::Text(
                        DeltaRopeBuilder::new()
                            .retain(pos as usize, Default::default())
                            .delete(len as usize)
                            .build(),
                    ),
                    ContainerType::List => Diff::List(
                        DeltaRopeBuilder::new()
                            .retain(pos as usize, Default::default())
                            .delete(len as usize)
                            .build(),
                    ),
                    _ => unreachable!("CompactInsert only records Text and List insertions"),
                };
                inverses.push((cid, op_diff));
            }
            RecordedOp::Diff {
                container,
                diff,
                map_written,
            } => {
                let cid = cid_of(container);
                if let Some((key, value)) = map_written {
                    map_values_after.insert((cid.clone(), key), value);
                }
                inverses.push((cid, diff));
            }
            // One op per movable list, so its snapshots are those of the span's
            // two ends; or only inserts, whose final places are read below.
            RecordedOp::Movable(record) => match movable.iter_mut().find(|r| r.cid == record.cid) {
                Some(r) if r.is_insert_only() && record.is_insert_only() => {
                    r.merge_inserts(&record)
                }
                Some(r) if r.is_delete_only() && record.is_delete_only() => {
                    if !r.merge_deletes(&record) {
                        return None;
                    }
                }
                Some(_) => return None,
                None => movable.push(record),
            },
            #[cfg(feature = "counter")]
            RecordedOp::Counter {
                container,
                increment,
            } => counter_ops.push((cid_of(container), increment)),
        }
    }
    let mut diff = crate::undo::DiffBatch::default();
    for (cid, op_diff) in inverses.into_iter().rev() {
        diff.compose_inverse_one(cid, op_diff);
    }
    // Mergeable children are found through their parent's marker, not their
    // own creation; whether a checkout reports them is not modelled here.
    if diff.cid_to_events.keys().any(|c| c.is_mergeable())
        || movable.iter().any(|r| r.cid.is_mergeable())
        || counter_ops.iter().any(|(c, _)| c.is_mergeable())
    {
        return None;
    }
    for record in movable.iter_mut().filter(|r| r.is_insert_only()) {
        let idx = arena.id_to_idx(&record.cid)?;
        record.refresh_after(state.container_state_mut(idx).as_movable_list_state()?);
    }
    let mut commit = crate::undo::CommitInverse {
        span,
        diff,
        compact_insert: None,
        map_values_after,
        movable,
        counter_ops,
    };
    // One op's delta is built normalized; only composing splits items.
    commit.finish(!single_op);
    if commit.diff.has_mixed_text_edits() {
        return None;
    }
    Some(crate::undo::LocalInverse::new(commit, deps))
}

/// The inverse of one local op, read from the state before the op is applied,
/// in the event coordinates right after it. `None` for ops whose inverse is not
/// recorded: undo then checks the document out, as before. It must equal what
/// undo gets from the checkout (`LoroDoc::undo_internal_with` checks this in
/// tests).
fn local_op_inverse(
    state: &mut DocState,
    container: ContainerIdx,
    raw_op: &RawOp,
    hint: &EventHint,
    doc: &Arc<LoroDocInner>,
) -> Option<RecordedOp> {
    match (container.get_type(), hint) {
        (ContainerType::Text, EventHint::InsertText { pos, event_len, .. }) => {
            return Some(RecordedOp::CompactInsert {
                container,
                pos: *pos,
                len: *event_len,
            });
        }
        (ContainerType::List, EventHint::InsertList { pos, len }) => {
            if let Ok(pos) = u32::try_from(*pos) {
                return Some(RecordedOp::CompactInsert {
                    container,
                    pos,
                    len: *len,
                });
            }
        }
        (ContainerType::MovableList, _) => {
            return movable_list_op_inverse(state, container, hint).map(RecordedOp::Movable)
        }
        #[cfg(feature = "counter")]
        (ContainerType::Counter, EventHint::Counter(x)) => {
            return Some(RecordedOp::Counter {
                container,
                increment: *x,
            })
        }
        _ => {}
    }
    let diff = local_op_inverse_diff(state, container, raw_op, hint, doc)?;
    let map_written = match hint {
        EventHint::Map { key, value } => Some((key.clone(), value.clone())),
        _ => None,
    };
    Some(RecordedOp::Diff {
        container,
        diff,
        map_written,
    })
}

/// A movable-list op: the identity record its undo plan reads
/// (`undo::movable_list`), before the op; [`RecordedInverse::finish`] reads
/// the rest after it.
///
/// [`RecordedInverse::finish`]: crate::undo::movable_list::RecordedInverse::finish
fn movable_list_op_inverse(
    state: &mut DocState,
    container: ContainerIdx,
    hint: &EventHint,
) -> Option<crate::undo::movable_list::RecordedInverse> {
    use crate::undo::movable_list::{ListEdit, RecordedInverse};
    let cid = state.arena.idx_to_id(container)?;
    let alive = state.get_reachable(&cid);
    let list = state
        .container_state_mut(container)
        .as_movable_list_state()?;
    let value_at = |i: usize| list.get(i, crate::state::IndexType::ForUser);
    let edit = match hint {
        EventHint::InsertList { len, pos } => ListEdit::Insert {
            pos: *pos,
            len: *len as usize,
        },
        EventHint::DeleteList(span) => {
            let start = span.start() as usize;
            // Restoring a child container revives it with its full state.
            if (start..start + span.len()).any(|i| value_at(i).is_none_or(|v| v.is_container())) {
                return None;
            }
            ListEdit::Delete {
                start,
                len: span.len(),
            }
        }
        EventHint::Move { from, to, .. } => ListEdit::Move {
            from: *from as usize,
            to: *to as usize,
        },
        EventHint::SetList { index, .. } => {
            if value_at(*index).is_none_or(|v| v.is_container()) {
                return None;
            }
            ListEdit::Set { index: *index }
        }
        _ => return None,
    };
    RecordedInverse::begin(list, cid, alive, edit)
}

fn local_op_inverse_diff(
    state: &mut DocState,
    container: ContainerIdx,
    raw_op: &RawOp,
    hint: &EventHint,
    doc: &Arc<LoroDocInner>,
) -> Option<Diff> {
    match (container.get_type(), hint) {
        (ContainerType::Text, EventHint::InsertText { pos, event_len, .. }) => Some(Diff::Text(
            DeltaRopeBuilder::new()
                .retain(*pos as usize, Default::default())
                .delete(*event_len as usize)
                .build(),
        )),
        (ContainerType::Text, EventHint::DeleteText { span, .. }) => {
            let RawOpContent::List(crate::container::list::list_op::ListOp::Delete(del)) =
                &raw_op.content
            else {
                return None;
            };
            let start = del.span.start() as usize;
            let end = start + del.span.len();
            let text = state
                .container_state_mut(container)
                .as_richtext_state_mut()?
                .slice_delta(start, end, crate::cursor::PosType::Entity)
                .ok()?;
            let mut delta: TextDiff = DeltaRopeBuilder::new()
                .retain(span.start() as usize, Default::default())
                .build();
            for (s, styles) in text {
                delta.push_insert(s.into(), styles.to_option_map().unwrap_or_default().into());
            }
            Some(Diff::Text(delta))
        }
        (ContainerType::List, EventHint::InsertList { len, pos }) => Some(Diff::List(
            DeltaRopeBuilder::new()
                .retain(*pos, Default::default())
                .delete(*len as usize)
                .build(),
        )),
        (ContainerType::List, EventHint::DeleteList(span)) => {
            let list = state.container_state_mut(container).as_list_state_mut()?;
            let start = span.start() as usize;
            let mut values = Vec::with_capacity(span.len());
            for i in start..start + span.len() {
                let v = list.get(i)?.clone();
                // Restoring a child container revives it with its full state.
                if v.is_container() {
                    return None;
                }
                values.push(ValueOrHandler::from_value(v, doc));
            }
            Some(Diff::List(
                DeltaRopeBuilder::new()
                    .retain(start, Default::default())
                    .insert_many(values, Default::default())
                    .build(),
            ))
        }
        (ContainerType::Map, EventHint::Map { key, .. }) => {
            let map = state.container_state_mut(container).as_map_state_mut()?;
            let old = match map.get_map_value(key) {
                None => ResolvedMapValue::new_unset(),
                Some(old) => {
                    if matches!(
                        old.value,
                        Some(LoroValue::Container(_) | LoroValue::Binary(_))
                    ) {
                        // A child container (or a mergeable marker) would be revived.
                        return None;
                    }
                    ResolvedMapValue {
                        value: old
                            .value
                            .clone()
                            .map(|v| ValueOrHandler::from_value(v, doc)),
                        idlp: IdLp::new(old.peer, old.lamp),
                    }
                }
            };
            Some(Diff::Map(
                ResolvedMapDelta::new().with_entry(key.clone(), old),
            ))
        }
        // A node created new, or moved between live parents. Deleting a node, or
        // moving it out of a deleted parent, revives (or retires) a subtree and
        // its metadata; those steps still check out.
        (ContainerType::Tree, EventHint::Tree(items)) if items.len() == 1 => {
            use crate::delta::TreeExternalDiff;
            let item = &items[0];
            let tree = state.container_state_mut(container).as_tree_state_mut()?;
            let action = match &item.action {
                TreeExternalDiff::Create { parent, index, .. } => {
                    if !tree.is_node_unexist(&item.target) {
                        return None;
                    }
                    TreeExternalDiff::Delete {
                        old_parent: *parent,
                        old_index: *index,
                    }
                }
                TreeExternalDiff::Move {
                    parent,
                    index,
                    old_parent,
                    old_index,
                    ..
                } => {
                    // Into a deleted parent the node is deleted too; undo then
                    // revives it (a new node with its metadata), as a checkout
                    // reports.
                    let into_deleted = match parent {
                        crate::TreeParentId::Node(p) => tree.is_node_deleted(p) != Some(false),
                        _ => false,
                    };
                    if into_deleted
                        || tree.parent(&item.target) != Some(*old_parent)
                        || tree.is_node_deleted(&item.target) != Some(false)
                    {
                        return None;
                    }
                    TreeExternalDiff::Move {
                        parent: *old_parent,
                        index: *old_index,
                        position: tree.get_position(&item.target)?,
                        old_parent: *parent,
                        old_index: *index,
                    }
                }
                // A leaf with empty metadata comes back with one `Create`; a
                // subtree or metadata would be revived with their full state.
                TreeExternalDiff::Delete {
                    old_parent,
                    old_index,
                } => {
                    if tree.parent(&item.target) != Some(*old_parent)
                        || tree.is_node_deleted(&item.target) != Some(false)
                        || tree
                            .children_num(&crate::TreeParentId::Node(item.target))
                            .unwrap_or(0)
                            > 0
                    {
                        return None;
                    }
                    let position = tree.get_position(&item.target)?;
                    let meta = item.target.associated_meta_container();
                    if let Some(meta_idx) = state.arena.id_to_idx(&meta) {
                        if !state.is_container_state_empty(meta_idx) {
                            return None;
                        }
                    }
                    TreeExternalDiff::Create {
                        parent: *old_parent,
                        index: *old_index,
                        position,
                    }
                }
            };
            let mut diff = TreeDiff::default();
            diff.diff.push(TreeDiffItem {
                target: item.target,
                action,
            });
            Some(Diff::Tree(diff))
        }
        _ => None,
    }
}
