use crate::encoding::json_schema::{encode_change, export_json_in_id_span};
pub use crate::encoding::ExportMode;
use crate::kv_store::SharedBlockCache;
use crate::memory::{
    heap_tracker_active, tracked_allocated_heap_bytes, tracked_peak_allocated_heap_bytes,
    wasm_linear_memory_bytes, DocMemoryStats, GlobalMemoryBudget, GlobalMemoryStats, MemoryBudget,
    DEFAULT_GLOBAL_BLOCK_CACHE_BYTES,
};
use crate::pre_commit::{FirstCommitFromPeerCallback, FirstCommitFromPeerPayload};
pub use crate::state::analyzer::{ContainerAnalysisInfo, DocAnalysis};
use crate::sync::{AtomicBool, AtomicUsize};
pub(crate) use crate::LoroDocInner;
use crate::{
    arena::{SharedArena, SharedArenaRollback},
    change::{Change, Timestamp},
    configure::{Configure, DefaultRandom, SecureRandomGenerator, StyleConfig},
    container::{
        idx::ContainerIdx, list::list_op::InnerListOp, richtext::config::StyleConfigMap,
        IntoContainerId,
    },
    cursor::{AbsolutePosition, CannotFindRelativePosition, Cursor, PosQueryResult},
    dag::{Dag, DagUtils},
    diff_calc::{DiffCalculator, DiffMode},
    encoding::{
        self, decode_snapshot, decode_snapshot_bytes, export_fast_snapshot, export_fast_updates,
        export_fast_updates_in_range, export_shallow_snapshot, export_snapshot_at,
        export_state_only_snapshot,
        json_schema::{encode_change_to_json, json::JsonSchema},
        parse_header_and_body, parse_header_and_body_bytes, EncodeMode, ImportBatchDetail,
        ImportBlobMetadata, ImportBlobOutcome, ImportStatus, ParsedHeaderAndBody,
        ParsedHeaderAndBodyBytes,
    },
    event::{str_to_path, EventTriggerKind, Index, InternalDocDiff},
    handler::{Handler, MovableListHandler, TextHandler, TreeHandler, ValueOrHandler},
    id::PeerID,
    json::JsonChange,
    op::InnerContent,
    oplog::{loro_dag::FrontiersNotIncluded, ImportedValues, OpLog},
    state::DocState,
    subscription::{LocalUpdateCallback, Observer, Subscriber},
    undo::DiffBatch,
    utils::subscription::{SubscriberSetWithQueue, Subscription},
    version::{Frontiers, ImVersionVector, VersionRange, VersionVectorDiff},
    ChangeMeta, DocDiff, HandlerTrait, InternalString, ListHandler, LoroDoc, LoroError, MapHandler,
    VersionVector,
};
use crate::{change::ChangeRef, lock::LockKind};
use crate::{lock::LoroMutexGuard, pre_commit::PreCommitCallback};
use crate::{
    lock::{LoroLockGroup, LoroMutex},
    txn::Transaction,
};
use bytes::Bytes;
use either::Either;

type CommitResult<'a> = (
    Option<CommitOptions>,
    Option<LoroMutexGuard<'a, Option<Transaction>>>,
);

#[cfg(test)]
thread_local! {
    pub(crate) static FAIL_APPLY_DIFF_AFTER_HANDLER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(crate) static RUN_APPLY_DIFF_HOOK_AFTER_NATIVE_PLAN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(crate) static FAIL_APPLY_DIFF_WITH_ARG_ERR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(crate) static AFTER_APPLY_DIFF_HANDLER: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn after_apply_diff_handler() -> LoroResult<()> {
    AFTER_APPLY_DIFF_HANDLER.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
    if FAIL_APPLY_DIFF_AFTER_HANDLER.with(|flag| flag.replace(false)) {
        if FAIL_APPLY_DIFF_WITH_ARG_ERR.with(|flag| flag.replace(false)) {
            return Err(LoroError::ArgErr("injected application error".into()));
        }
        Err(LoroError::OutOfBound {
            pos: usize::MAX,
            len: 0,
            info: "injected apply failure".into(),
        })
    } else {
        Ok(())
    }
}
use loro_common::{
    ContainerID, ContainerType, HasCounterSpan, HasIdSpan, HasLamportSpan, IdSpan, LoroEncodeError,
    LoroResult, LoroValue, MemoryLimitKind, ID,
};
use rle::HasLength;
use rustc_hash::{FxHashMap, FxHashSet};
use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::{hash_map::Entry, BinaryHeap},
    ops::ControlFlow,
    sync::{
        atomic::Ordering::{AcqRel, Acquire, Relaxed, Release},
        Arc,
    },
};
use tracing::{debug_span, info_span, instrument, warn};

use crate::utils::subscription::DeferredPanic;

impl Default for LoroDoc {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
thread_local! {
    /// Countdown of blobs `LoroDoc::import_batch` may import before panicking, standing
    /// in for the decode/apply panics that malformed remote data can trigger.
    static PANIC_AT_BATCH_IMPORT_BLOB: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn panic_at_batch_import_blob_for_test(blobs_to_import_first: usize) {
    PANIC_AT_BATCH_IMPORT_BLOB.with(|f| f.set(Some(blobs_to_import_first)));
}

#[cfg(test)]
fn take_panic_at_batch_import_blob_for_test() {
    PANIC_AT_BATCH_IMPORT_BLOB.with(|f| match f.get() {
        Some(0) => {
            f.set(None);
            panic!("batch import failpoint");
        }
        Some(n) => f.set(Some(n - 1)),
        None => {}
    });
}

impl std::fmt::Debug for LoroDocInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoroDoc")
            .field("config", &self.config)
            .field("auto_commit", &self.auto_commit)
            .field("detached", &self.detached)
            .finish()
    }
}

#[cfg(test)]
thread_local! {
    /// How many imports took the independent scalar-root fast path.
    static ISOLATED_FAST_PATH_HITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn isolated_fast_path_hits_for_test() -> usize {
    ISOLATED_FAST_PATH_HITS.with(|hits| hits.get())
}

impl LoroDoc {
    /// Run the provided closure within a commit barrier.
    ///
    /// This finalizes any pending auto-commit transaction first (preserving
    /// options across an empty txn), executes `f`, then renews the transaction
    /// (carrying preserved options) if auto-commit is enabled. This is the
    /// common implicit-commit pattern used by internal operations such as
    /// import/export/checkouts.
    #[inline]
    pub fn with_barrier<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let (options, guard) = self.implicit_commit_then_stop();
        // `f` may call user code, e.g. subscribers of an import's events. If it
        // panics, release the txn lock first, so the panic does not poison it.
        // A panic inside Loro may have poisoned another lock, which makes the
        // renewal panic too; `DeferredPanic` keeps the first one.
        let mut panic = DeferredPanic::default();
        let result = panic.run(f);
        drop(guard);
        panic.run(|| self.renew_txn_if_auto_commit(options));
        panic.resume();
        result.unwrap()
    }

    pub fn new() -> Self {
        let visible_op_count = Arc::new(AtomicUsize::new(0));
        let kv_block_cache = SharedBlockCache::default_per_doc();
        let global_flags = global_budget_flags();
        if global_flags != 0 {
            kv_block_cache.set_parent(Some(&global_pool().global_block_cache));
        }
        #[cfg(not(loom))]
        let pool_doc_id = NEXT_POOL_DOC_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        #[cfg(loom)]
        let pool_doc_id = if global_flags != 0 {
            NEXT_POOL_DOC_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        } else {
            0
        };
        let initial_tick = GLOBAL_ACCESS_TICK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let oplog = OpLog::new_with_cache(visible_op_count.clone(), kv_block_cache.clone());
        let arena = oplog.arena.clone();
        let config: Configure = oplog.configure.clone();
        let lock_group = LoroLockGroup::new();
        let global_txn = Arc::new(lock_group.new_lock(None, LockKind::Txn));
        let inner = Arc::new_cyclic(|w| {
            let state = DocState::new_arc_with_cache(
                w.clone(),
                arena.clone(),
                config.clone(),
                &lock_group,
                kv_block_cache.clone(),
            );
            LoroDocInner {
                oplog: Arc::new(lock_group.new_lock(oplog, LockKind::OpLog)),
                state,
                config,
                visible_op_count,
                detached: AtomicBool::new(false),
                auto_commit: AtomicBool::new(false),
                observer: Arc::new(Observer::new(arena.clone())),
                diff_calculator: Arc::new(
                    lock_group.new_lock(DiffCalculator::new(true), LockKind::DiffCalculator),
                ),
                txn: global_txn,
                arena,
                local_update_subs: SubscriberSetWithQueue::new(),
                peer_id_change_subs: SubscriberSetWithQueue::new(),
                pre_commit_subs: SubscriberSetWithQueue::new(),
                first_commit_from_peer_subs: SubscriberSetWithQueue::new(),
                undo_managers: crate::sync::Mutex::new(Vec::new()),
                active_undo_managers: std::sync::atomic::AtomicUsize::new(0),
                memory_budget: crate::sync::Mutex::new(MemoryBudget::default()),
                budget_flags: std::sync::atomic::AtomicU8::new(0),
                kv_block_cache,
                pool_doc_id,
                cached_doc_bytes: std::sync::atomic::AtomicUsize::new(0),
                cached_spilled_bytes: std::sync::atomic::AtomicUsize::new(0),
                stats_dirty: Arc::new(DocStatsDirty::new(pool_doc_id)),
                soft_backoff: SoftBackoff::new(),
                last_access_tick: std::sync::atomic::AtomicU64::new(initial_tick),
                user_handles: crate::sync::AtomicUsize::new(1),
            }
        });
        if pool_doc_id != 0 {
            global_pool().register(pool_doc_id, Arc::downgrade(&inner));
            // After registering: a mark pushes the id, and the refresh of a pushed id
            // must find the document in the registry.
            let dirty = &inner.stats_dirty;
            inner.txn.set_stats_dirty(dirty.clone());
            inner.oplog.set_stats_dirty(dirty.clone());
            inner.state.set_stats_dirty(dirty.clone());
            inner.diff_calculator.set_stats_dirty(dirty.clone());
            inner.arena.set_stats_dirty(dirty.clone());
            // `register` queued it (it starts dirty); the next estimate computes it.
        }
        LoroDoc::from_new_inner(inner)
    }

    pub fn fork(&self) -> Self {
        if self.is_detached() {
            return self
                .fork_at_inner(&self.state_frontiers(), false)
                .expect("fork_at on detached doc should not fail");
        }

        let snapshot = self
            .with_barrier(|| encoding::fast_snapshot::encode_snapshot_inner_for_fork(self))
            .expect("forking a valid document should encode a snapshot");
        let doc = Self::new();
        doc.set_memory_budget(self.memory_budget());
        doc.with_barrier(|| {
            encoding::fast_snapshot::decode_snapshot_inner(snapshot, &doc, Default::default())
        })
        .unwrap();
        doc.set_config(&self.config);
        self.copy_tree_fractional_index_settings_to(&doc);
        if self.auto_commit.load(std::sync::atomic::Ordering::Relaxed) {
            doc.start_auto_commit();
        }
        doc
    }

    /// Copies the trees' fractional-index settings, which are local configuration
    /// kept in their states, to a doc forked from this one. The fork has its own
    /// peer, so its jitter RNG is seeded afresh rather than copied.
    pub(crate) fn copy_tree_fractional_index_settings_to(&self, fork: &LoroDoc) {
        let settings = self.state.lock().tree_fractional_index_settings();
        if settings.is_empty() {
            return;
        }
        let settings: Vec<_> = settings
            .into_iter()
            .filter_map(|(idx, setting)| {
                let id = self.arena.get_container_id(idx)?;
                Some((fork.arena.register_container(&id), setting))
            })
            .collect();
        fork.state
            .lock()
            .set_tree_fractional_index_settings(&settings);
    }
    /// Enables editing of the document in detached mode.
    ///
    /// By default, the document cannot be edited in detached mode (after calling
    /// `detach` or checking out a version other than the latest). This method
    /// allows editing in detached mode.
    ///
    /// # Important Notes:
    ///
    /// - After enabling this mode, the document will use a different PeerID. Each
    ///   time you call checkout, a new PeerID will be used.
    /// - If you set a custom PeerID while this mode is enabled, ensure that
    ///   concurrent operations with the same PeerID are not possible. While the
    ///   PeerID has ops after the current version, writes return
    ///   [`LoroError::ConcurrentOpsWithSamePeerID`]; use another PeerID or check
    ///   out another version to write again.
    /// - On detached mode, importing will not change the state of the document.
    ///   It also doesn't change the version of the [DocState]. The changes will be
    ///   recorded into [OpLog] only. You need to call `checkout` to make it take effect.
    pub fn set_detached_editing(&self, enable: bool) {
        self.config.set_detached_editing(enable);
        if enable && self.is_detached() {
            self.with_barrier(|| {
                self.renew_peer_id();
            });
        }
    }

    /// Create a doc with auto commit enabled.
    #[inline]
    pub fn new_auto_commit() -> Self {
        let doc = Self::new();
        doc.start_auto_commit();
        doc
    }

    #[inline(always)]
    pub fn set_peer_id(&self, peer: PeerID) -> LoroResult<()> {
        if peer == PeerID::MAX {
            return Err(LoroError::InvalidPeerID);
        }
        let next_id = self.oplog.lock().next_id(peer);
        if self.auto_commit.load(Acquire) {
            let doc_state = self.state.lock();
            doc_state
                .peer
                .store(peer, std::sync::atomic::Ordering::Relaxed);

            if doc_state.is_in_txn() {
                drop(doc_state);
                // Use implicit-style barrier to avoid swallowing next-commit options
                self.with_barrier(|| {});
            }
            self.peer_id_change_subs.emit(&(), next_id);
            return Ok(());
        }

        let doc_state = self.state.lock();
        if doc_state.is_in_txn() {
            return Err(LoroError::TransactionError(
                "Cannot change peer id during transaction"
                    .to_string()
                    .into_boxed_str(),
            ));
        }

        doc_state
            .peer
            .store(peer, std::sync::atomic::Ordering::Relaxed);
        drop(doc_state);
        self.peer_id_change_subs.emit(&(), next_id);
        Ok(())
    }

    /// Renews the PeerID for the document.
    pub(crate) fn renew_peer_id(&self) {
        let mut peer_id = DefaultRandom.next_u64();
        while peer_id == PeerID::MAX {
            peer_id = DefaultRandom.next_u64();
        }
        self.set_peer_id(peer_id).unwrap();
    }

    /// Implicitly commit the cumulative auto-commit transaction.
    /// This method only has effect when `auto_commit` is true.
    ///
    /// Follow-ups: the caller is responsible for renewing the transaction
    /// as needed (e.g., via `renew_txn_if_auto_commit`). Prefer using
    /// `with_barrier(...)` for most internal flows to handle this safely.
    ///
    /// Empty-commit behavior: if the pending transaction is empty, the returned
    /// `Some(CommitOptions)` preserves next-commit options such as message and
    /// timestamp so they can carry into the renewed transaction. Transient
    /// labels like `origin` do not carry across an empty commit.
    #[inline]
    #[must_use]
    pub fn implicit_commit_then_stop(
        &self,
    ) -> (
        Option<CommitOptions>,
        LoroMutexGuard<'_, Option<Transaction>>,
    ) {
        // Implicit commit: preserve options on empty commit
        let (a, b) = self.commit_internal(CommitOptions::new().immediate_renew(false), true);
        (a, b.unwrap())
    }

    /// Commit the cumulative auto commit transaction.
    /// It will start the next one immediately
    ///
    /// It only returns Some(options_of_the_empty_txn) when the txn is empty
    #[inline]
    pub fn commit_then_renew(&self) -> Option<CommitOptions> {
        // Explicit commit: swallow options on empty commit
        self.commit_internal(CommitOptions::new().immediate_renew(true), false)
            .0
    }

    /// This method is called before the commit.
    /// It can be used to modify the change before it is committed.
    ///
    /// It return Some(txn) if the txn is None
    fn before_commit(&self) -> Option<LoroMutexGuard<'_, Option<Transaction>>> {
        let mut txn_guard = self.txn.lock();
        let Some(txn) = txn_guard.as_mut() else {
            return Some(txn_guard);
        };

        if txn.is_peer_first_appearance {
            txn.is_peer_first_appearance = false;
            drop(txn_guard);
            // First commit from a peer
            self.first_commit_from_peer_subs.emit(
                &(),
                FirstCommitFromPeerPayload {
                    peer: self.peer_id(),
                },
            );
        }

        None
    }

    /// Core implementation for committing the cumulative auto-commit transaction.
    ///
    /// - When `preserve_on_empty` is true (implicit commits like export/checkout),
    ///   commit options from an empty transaction are carried over to the next transaction
    ///   (except `origin`, which never carries across an empty commit).
    /// - When `preserve_on_empty` is false (explicit commits), commit options from an
    ///   empty transaction are swallowed and NOT carried over.
    ///
    /// A pre-commit callback that panics does not stop the commit; its panic is
    /// raised after the txn lock is released, so the lock is not poisoned.
    #[instrument(skip_all)]
    fn commit_internal(&self, config: CommitOptions, preserve_on_empty: bool) -> CommitResult<'_> {
        let mut panic = DeferredPanic::default();
        let ans = self
            .commit_internal_inner(config, preserve_on_empty, &mut panic, false)
            .expect("infallible commit does not enforce hard admission");
        if !panic.has_panic() {
            return ans;
        }
        drop(ans);
        panic.resume();
        unreachable!("resume raises the deferred panic")
    }

    fn commit_internal_inner(
        &self,
        config: CommitOptions,
        preserve_on_empty: bool,
        panic: &mut DeferredPanic,
        enforce_budget: bool,
    ) -> LoroResult<CommitResult<'_>> {
        if !self.auto_commit.load(Acquire) {
            let txn_guard = self.txn.lock();
            // if not auto_commit, nothing should happen
            // because the global txn is not used
            return Ok((None, Some(txn_guard)));
        }

        loop {
            if let Some(txn_guard) = self.before_commit() {
                return Ok((None, Some(txn_guard)));
            }

            let mut txn_guard = self.txn.lock();
            if enforce_budget {
                if let Some(txn) = txn_guard.as_mut() {
                    self.admit_transaction_commit(txn)?;
                }
            }
            let txn = txn_guard.take();
            let Some(mut txn) = txn else {
                return Ok((None, Some(txn_guard)));
            };
            let emit_on_commit = txn.take_emit_on_commit();
            if let Some(origin) = config.origin.clone() {
                txn.set_origin(origin);
            }

            if let Some(timestamp) = config.timestamp {
                txn.set_timestamp(timestamp);
            }

            if let Some(msg) = config.commit_msg.as_ref() {
                txn.set_msg(Some(msg.clone()));
            }

            let id_span = txn.id_span();
            let (mut options, post_commit) =
                txn.commit_deferring_panic(panic, emit_on_commit).unwrap();
            // Empty commit returns Some(options). We may preserve parts of it for implicit commits.
            if let Some(opts) = options.as_mut() {
                // `origin` is an event-only label and never carries across an empty commit
                if config.origin.is_some() {
                    opts.set_origin(None);
                }
                // For explicit commits, swallow options from empty commit entirely
                if !preserve_on_empty {
                    options = None;
                }
            }
            if config.immediate_renew && self.can_edit() {
                *txn_guard = self.auto_commit_txn(options.clone());
            }

            if emit_on_commit && self.has_post_commit_work(&post_commit.events, id_span) {
                drop(txn_guard);
                self.emit_post_commit(post_commit, id_span);
                if !config.immediate_renew {
                    txn_guard = self.txn.lock();
                    if txn_guard.is_some() {
                        // make sure that txn_guard is None when config.immediate_renew is false
                        continue;
                    }
                    return Ok((options, Some(txn_guard)));
                }
                return Ok((options, None));
            }

            return Ok((
                options,
                if !config.immediate_renew {
                    Some(txn_guard)
                } else {
                    None
                },
            ));
        }
    }

    /// Commit the cumulative auto commit transaction (explicit API).
    ///
    /// This is used by user-facing explicit commits. If the transaction is empty,
    /// any provided commit options are swallowed and will NOT carry over.
    #[instrument(skip_all)]
    pub fn commit_with(&self, config: CommitOptions) -> CommitResult<'_> {
        self.commit_internal(config, false)
    }

    /// Commits the current pending auto-commit transaction, enforcing any
    /// configured [`MemoryBudget::max_doc_bytes`] hard limit atomically.
    ///
    /// If committing would exceed `max_doc_bytes` even after soft and cold-state
    /// reclamation, pending uncommitted operations since the budget was armed
    /// are rolled back atomically without firing commit or diff subscribers and
    /// `Err(LoroError::MemoryLimitExceeded)` is returned.
    pub fn try_commit(&self) -> LoroResult<()> {
        self.try_commit_with(CommitOptions::default())
    }

    /// Commits the pending auto-commit transaction with options, enforcing any
    /// configured [`MemoryBudget::max_doc_bytes`] or global hard memory limit atomically.
    pub fn try_commit_with(&self, options: CommitOptions) -> LoroResult<()> {
        if self.txn.is_held_by_current_thread() {
            return Err(LoroError::DuplicatedTransactionError);
        }
        self.touch_access_tick();
        let mut panic = DeferredPanic::default();
        let result = self
            .commit_internal_inner(options, false, &mut panic, true)
            .map(|_| ());
        // Mapping drops any transaction guard before resuming user panics.
        panic.resume();
        result
    }

    fn admit_transaction_commit(&self, txn: &mut Transaction) -> LoroResult<()> {
        if txn.is_empty() {
            return Ok(());
        }
        if txn.doc_budget_limit.is_none()
            && !(global_budget_flags() & BUDGET_FLAG_MAX_DOC != 0
                && txn.budget_base_scope().is_some())
        {
            return Ok(());
        }
        if txn.has_rollback_scope() {
            return Err(LoroError::DuplicatedTransactionError);
        }
        let uncommitted = txn.estimated_uncommitted_bytes();
        self.enforce_memory_watermarks_with_txn_bytes(uncommitted, false);
        if let Err(err) = self.check_transaction_budget(txn, Some(txn.budget_scope_linear_bytes()))
        {
            let scope = txn
                .budget_base_scope()
                .expect("budgeted transaction has a base scope");
            assert!(
                txn.rollback_scope_is_intact(scope),
                "budget rollback scope must be intact"
            );
            self.rollback_transaction_scope_inner(txn, scope)?;
            self.refresh_cached_doc_bytes_unlocked();
            txn.doc_budget_limit = None;
            if self.has_max_doc_bytes() {
                self.arm_budget_scope_on_txn(txn);
            }
            return Err(err);
        }
        Ok(())
    }

    /// Set the commit message of the next commit
    pub fn set_next_commit_message(&self, message: &str) {
        let mut binding = self.txn.lock();
        let Some(txn) = binding.as_mut() else {
            return;
        };

        if message.is_empty() {
            txn.set_msg(None)
        } else {
            txn.set_msg(Some(message.into()))
        }
    }

    /// Set the origin of the next commit
    pub fn set_next_commit_origin(&self, origin: &str) {
        let mut txn = self.txn.lock();
        if let Some(txn) = txn.as_mut() {
            txn.set_origin(origin.into());
        }
    }

    /// Set the timestamp of the next commit
    pub fn set_next_commit_timestamp(&self, timestamp: Timestamp) {
        let mut txn = self.txn.lock();
        if let Some(txn) = txn.as_mut() {
            txn.set_timestamp(timestamp);
        }
    }

    /// Set the options of the next commit
    pub fn set_next_commit_options(&self, options: CommitOptions) {
        let mut txn = self.txn.lock();
        if let Some(txn) = txn.as_mut() {
            txn.set_options(options);
        }
    }

    /// Clear the options of the next commit
    pub fn clear_next_commit_options(&self) {
        let mut txn = self.txn.lock();
        if let Some(txn) = txn.as_mut() {
            txn.set_options(CommitOptions::new());
        }
    }

    /// Set whether to record the timestamp of each change. Default is `false`.
    ///
    /// If enabled, the Unix timestamp will be recorded for each change automatically.
    ///
    /// You can also set each timestamp manually when you commit a change.
    /// The timestamp manually set will override the automatic one.
    ///
    /// NOTE: Timestamps are forced to be in ascending order.
    /// If you commit a new change with a timestamp that is less than the existing one,
    /// the largest existing timestamp will be used instead.
    #[inline]
    pub fn set_record_timestamp(&self, record: bool) {
        self.config.set_record_timestamp(record);
    }

    pub fn decoded_value_cache_limit(&self) -> usize {
        self.config.decoded_value_cache_limit()
    }

    pub fn set_decoded_value_cache_limit(&self, limit: usize) {
        self.state.lock().store.set_decoded_value_cache_limit(limit);
    }

    /// Set the interval of mergeable changes, in seconds.
    ///
    /// If two continuous local changes are within the interval, they will be merged into one change.
    /// The default value is 1000 seconds.
    #[inline]
    pub fn set_change_merge_interval(&self, interval: i64) {
        self.config.set_merge_interval(interval);
    }

    pub fn can_edit(&self) -> bool {
        !self.is_detached() || self.config.detached_editing()
    }

    pub fn is_detached_editing_enabled(&self) -> bool {
        self.config.detached_editing()
    }

    #[inline]
    pub fn config_text_style(&self, text_style: StyleConfigMap) {
        self.config.text_style_config.write().map = text_style.map;
    }

    #[inline]
    pub fn config_default_text_style(&self, text_style: Option<StyleConfig>) {
        self.config.text_style_config.write().default_style = text_style;
    }
    pub fn from_snapshot(bytes: &[u8]) -> LoroResult<Self> {
        let doc = Self::new();
        let ParsedHeaderAndBody { mode, body, .. } = parse_header_and_body(bytes, true)?;
        if mode.is_snapshot() {
            doc.with_barrier(|| -> Result<(), LoroError> {
                decode_snapshot(&doc, mode, body, Default::default())?;
                Ok(())
            })?;
            Ok(doc)
        } else {
            Err(LoroError::DecodeError(
                "Invalid encode mode".to_string().into(),
            ))
        }
    }

    /// Like [`Self::from_snapshot`], but keeps references into `bytes` instead
    /// of copying the snapshot. The doc (and its kv stores and block caches)
    /// may hold slices of `bytes` for its whole lifetime, so the caller must
    /// not mutate the underlying memory; see `context/internal-encoding.md`.
    pub fn from_snapshot_bytes(bytes: Bytes) -> LoroResult<Self> {
        let doc = Self::new();
        let ParsedHeaderAndBody { mode, body, .. } = parse_header_and_body(&bytes, true)?;
        if mode.is_snapshot() {
            let body = bytes.slice_ref(body);
            doc.with_barrier(|| -> Result<(), LoroError> {
                decode_snapshot_bytes(&doc, mode, body, Default::default())?;
                Ok(())
            })?;
            Ok(doc)
        } else {
            Err(LoroError::DecodeError(
                "Invalid encode mode".to_string().into(),
            ))
        }
    }

    /// Can a snapshot initialize state/history, preserving any parked changes?
    #[inline(always)]
    pub fn can_reset_with_snapshot(&self) -> bool {
        let oplog = self.oplog.lock();
        if oplog.batch_importing {
            return false;
        }

        if self.is_detached() {
            return false;
        }

        oplog.dag().is_empty() && self.state.lock().can_import_snapshot()
    }

    /// Whether [OpLog] and [DocState] are detached.
    ///
    /// If so, the document is in readonly mode by default and importing will not change the state of the document.
    /// It also doesn't change the version of the [DocState]. The changes will be recorded into [OpLog] only.
    /// You need to call `checkout` to make it take effect.
    #[inline(always)]
    pub fn is_detached(&self) -> bool {
        self.detached.load(Acquire)
    }

    pub(crate) fn set_detached(&self, detached: bool) {
        self.detached.store(detached, Release);
    }

    #[inline(always)]
    pub fn peer_id(&self) -> PeerID {
        self.state
            .lock()
            .peer
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn detach(&self) {
        self.with_barrier(|| self.set_detached(true));
    }

    /// [`Self::try_attach`] for a caller that cannot handle an error.
    ///
    /// # Panics
    ///
    /// Where that method returns an error, once no lock is held, so the doc stays usable.
    #[inline(always)]
    pub fn attach(&self) {
        self.checkout_to_latest()
    }

    /// Attach the state to the latest version, as [`Self::try_checkout_to_latest`].
    #[inline(always)]
    pub fn try_attach(&self) -> LoroResult<()> {
        self.try_checkout_to_latest()
    }

    /// Get the timestamp of the current state.
    /// It's the last edit time of the [DocState].
    pub fn state_timestamp(&self) -> Timestamp {
        // Acquire locks in correct order: read frontiers first, then query OpLog.
        let f = { self.state.lock().frontiers.clone() };
        self.oplog.lock().get_timestamp_of_version(&f)
    }

    #[inline(always)]
    pub fn app_state(&self) -> &Arc<LoroMutex<DocState>> {
        &self.state
    }

    #[inline]
    pub fn get_state_deep_value(&self) -> LoroValue {
        self.state.lock().get_deep_value()
    }

    #[inline(always)]
    pub fn oplog(&self) -> &Arc<LoroMutex<OpLog>> {
        &self.oplog
    }

    #[inline(always)]
    pub fn import(&self, bytes: &[u8]) -> Result<ImportStatus, LoroError> {
        let s = debug_span!("import", peer = self.peer_id());
        let _e = s.enter();
        self.import_with(bytes, Default::default())
    }

    /// Like [`Self::import`], taking owned ref-counted [`bytes::Bytes`] to avoid
    /// copying the input buffer during decoding.
    #[inline(always)]
    pub fn import_bytes(&self, bytes: bytes::Bytes) -> Result<ImportStatus, LoroError> {
        let s = debug_span!("import_bytes", peer = self.peer_id());
        let _e = s.enter();
        self.import_with_bytes(bytes, Default::default())
    }

    #[inline]
    pub fn import_with(
        &self,
        bytes: &[u8],
        origin: InternalString,
    ) -> Result<ImportStatus, LoroError> {
        self.import_with_history_mode(bytes, origin, false)
    }

    /// Like [`Self::import_with`], taking owned ref-counted [`bytes::Bytes`] to avoid
    /// copying the input buffer during decoding.
    #[inline]
    pub fn import_with_bytes(
        &self,
        bytes: bytes::Bytes,
        origin: InternalString,
    ) -> Result<ImportStatus, LoroError> {
        self.import_with_history_mode_bytes(bytes, origin, false)
    }

    /// Import binary bytes. `json_lossy` accepts only the equivalences a JSON hop
    /// can introduce; the default binary path passes `false`.
    pub fn import_with_history_mode(
        &self,
        bytes: &[u8],
        origin: InternalString,
        json_lossy: bool,
    ) -> Result<ImportStatus, LoroError> {
        self.import_input_with_history_mode(ImportInput::Slice(bytes), origin, json_lossy)
    }

    /// Like [`Self::import_with_history_mode`], taking owned ref-counted [`bytes::Bytes`].
    pub fn import_with_history_mode_bytes(
        &self,
        bytes: bytes::Bytes,
        origin: InternalString,
        json_lossy: bool,
    ) -> Result<ImportStatus, LoroError> {
        self.import_input_with_history_mode(ImportInput::Bytes(bytes), origin, json_lossy)
    }

    fn import_input_with_history_mode(
        &self,
        input: ImportInput<'_>,
        origin: InternalString,
        json_lossy: bool,
    ) -> Result<ImportStatus, LoroError> {
        self.check_import_not_nested()?;
        let values = imported_values_for_mode(json_lossy);
        self.with_barrier(|| self._import_with_input(input, origin, values))
    }

    #[tracing::instrument(skip_all)]
    fn _import_with_input(
        &self,
        input: ImportInput<'_>,
        origin: InternalString,
        values: ImportedValues,
    ) -> Result<ImportStatus, LoroError> {
        let result = self.import_without_emitting_input(input, origin, values);
        // The import rollback scope has closed, so history compaction is now
        // safe even for budgets that configure only soft watermarks.
        self.enforce_memory_watermarks_with_txn_bytes(0, false);
        self.emit_events();
        result
    }

    fn import_without_emitting_input(
        &self,
        input: ImportInput<'_>,
        origin: InternalString,
        values: ImportedValues,
    ) -> Result<ImportStatus, LoroError> {
        ensure_cov::notify_cov("loro_internal::import");
        self.touch_access_tick();
        match &input {
            ImportInput::Slice(bytes) => self.check_import_blob_limit(bytes.len())?,
            ImportInput::Bytes(bytes) => self.check_import_blob_limit(bytes.len())?,
        }
        match input {
            ImportInput::Slice(bytes) => {
                let parsed = parse_header_and_body(bytes, true)?;
                loro_common::info!("Importing with mode={:?}", &parsed.mode);
                match parsed.mode {
                    EncodeMode::OutdatedRle => {
                        let s = tracing::span!(
                            tracing::Level::INFO,
                            "Import updates ",
                            peer = self.peer_id()
                        );
                        let _e = s.enter();
                        self.update_oplog_and_apply_delta_to_state_if_needed(
                            |oplog| oplog.decode(parsed, values),
                            origin,
                        )
                    }
                    EncodeMode::OutdatedSnapshot => {
                        if self.can_reset_with_snapshot() {
                            loro_common::info!("Init by snapshot {}", self.peer_id());
                            decode_snapshot(self, parsed.mode, parsed.body, origin)
                        } else {
                            self.update_oplog_and_apply_delta_to_state_if_needed(
                                |oplog| oplog.decode(parsed, values),
                                origin,
                            )
                        }
                    }
                    EncodeMode::FastSnapshot => {
                        if self.can_reset_with_snapshot() {
                            ensure_cov::notify_cov("loro_internal::import::snapshot");
                            loro_common::info!("Init by fast snapshot {}", self.peer_id());
                            self.init_snapshot_preserving_pending(
                                parsed.mode,
                                parsed.body,
                                origin,
                                values,
                            )
                        } else {
                            self.import_changes_and_apply_delta_to_state_if_needed(
                                |oplog| encoding::decode_oplog_changes(oplog, parsed, values),
                                values,
                                origin,
                            )
                        }
                    }
                    EncodeMode::FastUpdates => self
                        .import_changes_and_apply_delta_to_state_if_needed(
                            |oplog| encoding::decode_oplog_changes(oplog, parsed, values),
                            values,
                            origin,
                        ),
                    EncodeMode::Auto => unreachable!(),
                }
            }
            ImportInput::Bytes(bytes) => {
                let parsed = parse_header_and_body_bytes(bytes, true)?;
                loro_common::info!("Importing with mode={:?}", &parsed.mode);
                match parsed.mode {
                    EncodeMode::OutdatedRle => {
                        let s = tracing::span!(
                            tracing::Level::INFO,
                            "Import updates ",
                            peer = self.peer_id()
                        );
                        let _e = s.enter();
                        let borrowed = parsed_as_borrowed(&parsed);
                        self.update_oplog_and_apply_delta_to_state_if_needed(
                            |oplog| oplog.decode(borrowed, values),
                            origin,
                        )
                    }
                    EncodeMode::OutdatedSnapshot => {
                        if self.can_reset_with_snapshot() {
                            loro_common::info!("Init by snapshot {}", self.peer_id());
                            decode_snapshot_bytes(self, parsed.mode, parsed.body, origin)
                        } else {
                            let borrowed = parsed_as_borrowed(&parsed);
                            self.update_oplog_and_apply_delta_to_state_if_needed(
                                |oplog| oplog.decode(borrowed, values),
                                origin,
                            )
                        }
                    }
                    EncodeMode::FastSnapshot => {
                        if self.can_reset_with_snapshot() {
                            ensure_cov::notify_cov("loro_internal::import::snapshot");
                            loro_common::info!("Init by fast snapshot {}", self.peer_id());
                            self.init_snapshot_preserving_pending_bytes(
                                parsed.mode,
                                parsed.body,
                                origin,
                                values,
                            )
                        } else {
                            self.import_changes_and_apply_delta_to_state_if_needed(
                                |oplog| encoding::decode_oplog_changes_bytes(oplog, parsed, values),
                                values,
                                origin,
                            )
                        }
                    }
                    EncodeMode::FastUpdates => self
                        .import_changes_and_apply_delta_to_state_if_needed(
                            |oplog| encoding::decode_oplog_changes_bytes(oplog, parsed, values),
                            values,
                            origin,
                        ),
                    EncodeMode::Auto => unreachable!(),
                }
            }
        }
    }

    fn init_snapshot_preserving_pending(
        &self,
        mode: EncodeMode,
        body: &[u8],
        origin: InternalString,
        values: ImportedValues,
    ) -> LoroResult<ImportStatus> {
        self.init_snapshot_preserving_pending_with(origin, values, |doc, origin| {
            decode_snapshot(doc, mode, body, origin)
        })
    }

    fn init_snapshot_preserving_pending_bytes(
        &self,
        mode: EncodeMode,
        body: bytes::Bytes,
        origin: InternalString,
        values: ImportedValues,
    ) -> LoroResult<ImportStatus> {
        self.init_snapshot_preserving_pending_with(origin, values, |doc, origin| {
            decode_snapshot_bytes(doc, mode, body, origin)
        })
    }

    /// Bootstrap state and history together, then replay parked changes against
    /// that exact snapshot version. On error restore the original parking claims
    /// as well as empty handles; no queued snapshot event may escape.
    fn init_snapshot_preserving_pending_with(
        &self,
        origin: InternalString,
        values: ImportedValues,
        decode_fn: impl FnOnce(&Self, InternalString) -> LoroResult<ImportStatus>,
    ) -> LoroResult<ImportStatus> {
        let (checkpoint, empty_containers, pending) = {
            let mut oplog = self.oplog.lock();
            let mut state = self.state.lock();
            (
                oplog.arena.checkpoint_for_rollback(),
                state.store.iter_all_container_ids().collect::<Vec<_>>(),
                std::mem::take(&mut oplog.pending_changes),
            )
        };
        let was_detached = self.is_detached();
        let linear_before = wasm_linear_memory_bytes();
        let result = (|| {
            let mut status = decode_fn(self, origin.clone())?;
            // A replay-only shallow snapshot's decoder temporarily attaches to
            // its retained head. The batch still owns the final checkout.
            self.set_detached(was_detached);
            if !pending.is_empty() {
                let changes = pending.replay_changes();
                self.import_changes_and_apply_delta_to_state_if_needed(
                    |_| Ok(changes),
                    values,
                    origin,
                )?;
                let oplog = self.oplog.lock();
                status.success = VersionRange::from_vv(oplog.vv());
                status.pending = non_empty(oplog.pending_changes.version_range());
            }
            if !self.oplog.lock().batch_importing && self.has_max_doc_bytes() {
                self.enforce_memory_watermarks_with_txn_bytes(0, false);
                self.check_doc_budget_with_txn_bytes_and_linear(0, Some(linear_before))?;
            }
            Ok(status)
        })();
        self.set_detached(was_detached);
        if result.is_err() {
            let mut oplog = self.oplog.lock();
            let mut state = self.state.lock();
            state.reset_to_empty_for_failed_snapshot_import();
            oplog.reset_to_empty_for_failed_snapshot_import(checkpoint, &mut state);
            oplog.pending_changes = pending;
            for id in empty_containers {
                state.ensure_container(&id);
            }
            oplog.set_block_cache(self.kv_block_cache.clone());
            state.set_block_cache(self.kv_block_cache.clone());
            drop(state);
            drop(oplog);
            *self.diff_calculator.lock() = DiffCalculator::new(true);
            if global_budget_flags() != 0 {
                self.refresh_cached_doc_bytes_unlocked();
            }
        } else if global_budget_flags() != 0 && !self.oplog.lock().batch_importing {
            self.refresh_cached_doc_bytes_unlocked();
        }
        result
    }

    /// An import called back on a thread that holds the transaction lock, from an
    /// event or pre-commit callback, would wait for itself. A transaction or another
    /// import is running there, so this is the same refusal as [`Self::check_not_in_txn`].
    fn check_import_not_nested(&self) -> LoroResult<()> {
        if self.txn.is_held_by_current_thread() {
            return Err(LoroError::ImportWhenInTxn);
        }
        Ok(())
    }

    /// An open transaction has ops in the version vector that the change store does
    /// not have, and it took its next id before the import. Comparing with history
    /// cannot see them, and changes that park now would hold ids it is about to commit.
    /// Barriers stop the auto-commit transaction, so this is a manual [`Transaction`].
    /// Call it with the op log locked: a transaction starts under that lock, so none
    /// can start before the import ends. See `context/import-peer-id-reuse.md`.
    fn check_not_in_txn(state: &LoroMutex<DocState>) -> LoroResult<()> {
        if state.lock().is_in_txn() {
            return Err(LoroError::ImportWhenInTxn);
        }
        Ok(())
    }

    #[tracing::instrument(skip_all)]
    pub(crate) fn update_oplog_and_apply_delta_to_state_if_needed(
        &self,
        f: impl FnOnce(&mut OpLog) -> Result<ImportStatus, LoroError>,
        origin: InternalString,
    ) -> Result<ImportStatus, LoroError> {
        let mut oplog = self.oplog.lock();
        Self::check_not_in_txn(&self.state)?;
        // `import_batch` opens a rollback scope that spans the whole batch. Rollback
        // scopes cannot nest (the inner `begin` would clobber the outer journal), so
        // when one is already open we let the owner decide what to keep: the batch
        // scope rolls the whole batch back if its final reattach fails, and commits
        // everything otherwise.
        let owns_rollback = !oplog.has_import_rollback();
        let linear_before = wasm_linear_memory_bytes();
        if owns_rollback {
            oplog.begin_import_rollback();
        }
        if !self.is_detached() {
            let old_vv = oplog.vv().clone();
            let old_frontiers = oplog.frontiers().clone();
            let result = f(&mut oplog);
            if &old_vv != oplog.vv() {
                let computed = DiffCalculator::new(false).try_calc_diff_internal(
                    &oplog,
                    &old_vv,
                    &old_frontiers,
                    oplog.vv(),
                    oplog.dag.get_frontiers(),
                    None,
                );
                let (diff, diff_mode) = match computed {
                    Ok(diff) => diff,
                    Err(err) => {
                        oplog.rollback_owned_import(owns_rollback, &mut self.state.lock());
                        return Err(err);
                    }
                };
                let mut state = self.state.lock();
                // A quota may reject the import after the state changed; record
                // how to undo it (see `state/undo_log.rs`).
                let quota = owns_rollback && self.has_max_doc_bytes();
                if quota {
                    state.begin_import_undo();
                }
                let applied = state.apply_diff(
                    InternalDocDiff {
                        origin,
                        diff: (diff).into(),
                        by: EventTriggerKind::Import,
                        new_version: Cow::Owned(oplog.frontiers().clone()),
                        local_inverse: None,
                    },
                    diff_mode,
                );
                let undo = state.take_import_undo();
                if let Err(e) = applied {
                    oplog.rollback_owned_import(owns_rollback, &mut state);
                    return Err(e);
                }
                if let Some(undo) = undo {
                    self.enforce_watermarks_locked(&oplog, &mut state, 0, false);
                    if let Err(budget_err) = self.check_doc_budget_locked_with_linear(
                        &oplog,
                        &state,
                        0,
                        Some(linear_before),
                    ) {
                        state.undo_import(undo, old_frontiers.clone());
                        oplog.rollback_owned_import(owns_rollback, &mut state);
                        *self.diff_calculator.lock() = DiffCalculator::new(true);
                        return Err(budget_err);
                    }
                } else if global_budget_flags() != 0 {
                    let _ = self.memory_stats_locked(&oplog, &state, 0);
                }
            }
            match result {
                Ok(result) => {
                    oplog.commit_owned_import_rollback(owns_rollback);
                    Ok(result)
                }
                Err(e) => {
                    // Some import errors are reported after a valid prefix has
                    // been inserted into the oplog. Keep that prefix if it was
                    // also applied to state; rollback is for failed state apply
                    // or decode errors that made no visible oplog progress.
                    let keep_prefix = &old_vv != oplog.vv();
                    if keep_prefix {
                        oplog.commit_owned_import_rollback(owns_rollback);
                    } else {
                        oplog.rollback_owned_import(owns_rollback, &mut self.state.lock());
                    }
                    Err(e)
                }
            }
        } else {
            match f(&mut oplog) {
                Ok(result) => {
                    oplog.commit_owned_import_rollback(owns_rollback);
                    Ok(result)
                }
                Err(e) => {
                    oplog.rollback_owned_import(owns_rollback, &mut self.state.lock());
                    Err(e)
                }
            }
        }
    }

    #[tracing::instrument(skip_all)]
    pub(crate) fn import_changes_and_apply_delta_to_state_if_needed(
        &self,
        decode_changes: impl FnOnce(&mut OpLog) -> Result<Vec<Change>, LoroError>,
        values: ImportedValues,
        origin: InternalString,
    ) -> Result<ImportStatus, LoroError> {
        let linear_before = wasm_linear_memory_bytes();
        let mut oplog = self.oplog.lock();
        Self::check_not_in_txn(&self.state)?;
        oplog.check_history_parsable()?;
        let arena_checkpoint = oplog.arena.checkpoint_for_rollback();
        let changes = match decode_changes(&mut oplog)
            .and_then(|changes| oplog.check_and_trim_known_part_of_changes(changes, values))
        {
            Ok(changes) => changes,
            Err(e) => {
                oplog.rollback_arena(arena_checkpoint, &mut self.state.lock());
                return Err(e);
            }
        };

        // Read once: attach/detach can change the flag without the oplog lock.
        // The apply branch must use the same mode as preflight's rollback decision.
        let detached = self.is_detached();
        let preflight = match oplog.preflight_import_changes(&changes, detached) {
            Ok(preflight) => preflight,
            Err(err) => {
                oplog.rollback_arena(arena_checkpoint, &mut self.state.lock());
                return Err(err);
            }
        };
        if preflight.has_deps_before_shallow_root && (detached || !preflight.applies_to_dag) {
            oplog.rollback_arena(arena_checkpoint, &mut self.state.lock());
            return Err(LoroError::ImportUpdatesThatDependsOnOutdatedVersion);
        }

        if detached {
            // An enclosing `import_batch` scope validates the whole batch before it
            // reattaches (`BatchImportGuard::finish`).
            let owns_rollback = !oplog.has_import_rollback()
                && (preflight.needs_state_apply_rollback
                    || oplog.has_max_pending_bytes()
                    || self.has_max_doc_bytes());
            if owns_rollback {
                oplog.begin_import_rollback_with_arena(arena_checkpoint);
            }
            let result = match encoding::apply_decoded_changes_to_oplog(&mut oplog, changes) {
                Ok(result) => result,
                Err(err) => {
                    if owns_rollback {
                        oplog.rollback_import(&mut self.state.lock());
                    }
                    return Err(err);
                }
            };
            if owns_rollback {
                if let Err(e) = oplog.validate_op_refs_in_import_scope() {
                    oplog.rollback_import(&mut self.state.lock());
                    return Err(e);
                }
                if self.has_max_doc_bytes() {
                    let mut state = self.state.lock();
                    self.enforce_watermarks_locked(&oplog, &mut state, 0, false);
                    if let Err(e) = self.check_doc_budget_locked_with_linear(
                        &oplog,
                        &state,
                        0,
                        Some(linear_before),
                    ) {
                        oplog.rollback_import(&mut state);
                        let _ = self.memory_stats_locked(&oplog, &state, 0);
                        return Err(e);
                    }
                } else if global_budget_flags() != 0 {
                    let state = self.state.lock();
                    let _ = self.memory_stats_locked(&oplog, &state, 0);
                }
                oplog.commit_import_rollback();
            }
            if result.has_deps_before_shallow_root {
                return Err(LoroError::ImportUpdatesThatDependsOnOutdatedVersion);
            }

            return Ok(result.status);
        }

        if !preflight.applies_to_dag {
            let pending_root_containers =
                match pending_root_containers_to_materialize(&oplog, &changes) {
                    Ok(ids) => ids,
                    Err(err) => {
                        oplog.rollback_arena(arena_checkpoint, &mut self.state.lock());
                        return Err(err);
                    }
                };
            let owns_rollback = !oplog.has_import_rollback()
                && (oplog.has_max_pending_bytes() || self.has_max_doc_bytes());
            let fallback_arena = if owns_rollback {
                oplog.begin_import_rollback_with_arena(arena_checkpoint);
                None
            } else if !oplog.has_import_rollback() {
                Some(arena_checkpoint)
            } else {
                None
            };
            let result = match encoding::apply_decoded_changes_to_oplog(&mut oplog, changes) {
                Ok(result) => result,
                Err(err) => {
                    if owns_rollback {
                        oplog.rollback_import(&mut self.state.lock());
                    } else if let Some(arena_cp) = fallback_arena {
                        oplog.rollback_arena(arena_cp, &mut self.state.lock());
                    }
                    return Err(err);
                }
            };
            if result.has_deps_before_shallow_root {
                if owns_rollback {
                    oplog.commit_import_rollback();
                }
                // Preflight rejected this import's own obsolete changes. Only
                // pre-existing pending changes were dropped here; newly parked
                // changes still reference this import's arena allocations.
                return Err(LoroError::ImportUpdatesThatDependsOnOutdatedVersion);
            }

            if !pending_root_containers.is_empty() || (owns_rollback && self.has_max_doc_bytes()) {
                let mut state = self.state.lock();
                if owns_rollback && self.has_max_doc_bytes() {
                    state.store.begin_created_journal();
                }
                for id in pending_root_containers {
                    state.ensure_container(&id);
                }
                if owns_rollback && self.has_max_doc_bytes() {
                    self.enforce_watermarks_locked(&oplog, &mut state, 0, false);
                    if let Err(e) = self.check_doc_budget_locked_with_linear(
                        &oplog,
                        &state,
                        0,
                        Some(linear_before),
                    ) {
                        let created = state.store.take_created_journal();
                        state.store.remove_created(&created);
                        oplog.rollback_import(&mut state);
                        let _ = self.memory_stats_locked(&oplog, &state, 0);
                        return Err(e);
                    }
                    let _ = state.store.take_created_journal();
                } else if !oplog.batch_importing && global_budget_flags() != 0 {
                    let _ = self.memory_stats_locked(&oplog, &state, 0);
                }
            }
            if owns_rollback {
                oplog.commit_import_rollback();
            }

            return Ok(result.status);
        }

        let old_vv = Arc::new(oplog.vv().clone());
        let old_frontiers = oplog.frontiers().clone();
        // Checked before the changes are applied, while the store still holds only old history.
        let isolated_batch = isolated_scalar_root_batch(&oplog, &changes).filter(|(_, names)| {
            let mut state = self.state.lock();
            names.iter().all(|name| {
                let id = ContainerID::new_root(name, ContainerType::Map);
                !state.store.contains_id(&id)
            })
        });
        let rollback_enabled = preflight.needs_state_apply_rollback
            || oplog.change_store().may_have_unparsed_bodies()
            || oplog.has_max_pending_bytes()
            || self.has_max_doc_bytes();
        if rollback_enabled {
            oplog.begin_import_rollback_with_version(arena_checkpoint, old_vv.clone());
        }

        let result = match encoding::apply_decoded_changes_to_oplog(&mut oplog, changes) {
            Ok(result) => result,
            Err(err) => {
                oplog.rollback_owned_import(rollback_enabled, &mut self.state.lock());
                return Err(err);
            }
        };
        if old_vv.as_ref() != oplog.vv() {
            // The preflight enables rollback whenever the imported or unlocked
            // pending changes hold movable-list or text ops, so other imports skip the scan.
            if rollback_enabled {
                if let Err(e) = oplog.validate_op_refs_in_import_scope() {
                    oplog.rollback_import(&mut self.state.lock());
                    return Err(e);
                }
            }
            let mut applied_direct = false;
            // Recording and a byte budget need the diff (events, import undo).
            // A quiet linear append can write the container state directly.
            if !oplog.batch_importing && !self.has_max_doc_bytes() {
                let mut state = self.state.lock();
                if state.frontiers == old_frontiers && !state.is_recording() {
                    match crate::state::direct_import::try_apply(
                        &oplog,
                        &mut state,
                        &old_frontiers,
                        &old_vv,
                        rollback_enabled,
                        origin.as_str(),
                    ) {
                        Ok(true) => {
                            if rollback_enabled {
                                let _ = state.store.take_created_journal();
                            }
                            if global_budget_flags() != 0 {
                                let _ = self.memory_stats_locked(&oplog, &state, 0);
                            }
                            applied_direct = true;
                        }
                        Ok(false) => {}
                        Err(err) => {
                            oplog.rollback_owned_import(rollback_enabled, &mut state);
                            return Err(err);
                        }
                    }
                }
            }
            if !applied_direct {
                let mut calc = DiffCalculator::new(false);
                // Applying may have unlocked pending changes; the isolated fast path is only valid
                // when exactly the candidate batch was appended on top of the old version.
                let isolated_batch = isolated_batch.filter(|(component_vv, _)| {
                    component_vv
                        .iter()
                        .all(|(peer, end)| oplog.vv().get(peer) == Some(end))
                        && oplog.vv().iter().all(|(peer, end)| {
                            let old_end = old_vv.get(peer).copied().unwrap_or(0);
                            let component_end = component_vv.get(peer).copied().unwrap_or(0);
                            *end == old_end.max(component_end)
                        })
                });
                let computed = (|| -> LoroResult<_> {
                    let (diff, diff_mode) = if let Some((component_vv, _)) = isolated_batch {
                        #[cfg(test)]
                        ISOLATED_FAST_PATH_HITS.with(|hits| hits.set(hits.get() + 1));
                        let component_frontiers = oplog.dag.try_vv_to_frontiers(&component_vv)?;
                        let (diff, _) = calc.try_calc_diff_internal(
                            &oplog,
                            &VersionVector::default(),
                            &Frontiers::default(),
                            &component_vv,
                            &component_frontiers,
                            None,
                        )?;
                        (diff, DiffMode::Import)
                    } else {
                        calc.try_calc_diff_internal(
                            &oplog,
                            &old_vv,
                            &old_frontiers,
                            oplog.vv(),
                            oplog.dag.get_frontiers(),
                            None,
                        )?
                    };
                    oplog.check_history_parsable()?;
                    Ok((diff, diff_mode))
                })();
                let (diff, diff_mode) = match computed {
                    Ok(diff) => diff,
                    Err(err) => {
                        oplog.rollback_owned_import(rollback_enabled, &mut self.state.lock());
                        return Err(err);
                    }
                };
                let mut state = self.state.lock();
                let diff = match recalc_in_checkout_mode_if_needed(
                    &mut state,
                    &oplog,
                    &old_vv,
                    &old_frontiers,
                    diff,
                ) {
                    Ok(diff) => diff,
                    Err(err) => {
                        oplog.rollback_owned_import(rollback_enabled, &mut state);
                        return Err(err);
                    }
                };
                if rollback_enabled {
                    state.store.begin_created_journal();
                }
                // See the other import path: record how to undo it under a quota.
                if rollback_enabled && !oplog.batch_importing && self.has_max_doc_bytes() {
                    state.begin_import_undo();
                }
                let applied = state.apply_diff(
                    InternalDocDiff {
                        origin,
                        diff: (diff).into(),
                        by: EventTriggerKind::Import,
                        new_version: Cow::Owned(oplog.frontiers().clone()),
                        local_inverse: None,
                    },
                    diff_mode,
                );
                let undo = state.take_import_undo();
                if let Err(e) = applied {
                    if rollback_enabled {
                        let created = state.store.take_created_journal();
                        state.store.remove_created(&created);
                        oplog.rollback_import(&mut state);
                        return Err(e);
                    }

                    panic!("state apply returned Err for import without rollback guard: {e}");
                }
                materialize_touched_roots(&mut state, &calc, &oplog.arena);
                drop(calc);
                if rollback_enabled {
                    if let Some(undo) = undo {
                        self.enforce_watermarks_locked(&oplog, &mut state, 0, false);
                        if let Err(budget_err) = self.check_doc_budget_locked_with_linear(
                            &oplog,
                            &state,
                            0,
                            Some(linear_before),
                        ) {
                            state.undo_import(undo, old_frontiers.clone());
                            let created = state.store.take_created_journal();
                            state.store.remove_created(&created);
                            oplog.rollback_import(&mut state);
                            *self.diff_calculator.lock() = DiffCalculator::new(true);
                            let _ = self.memory_stats_locked(&oplog, &state, 0);
                            return Err(budget_err);
                        }
                    } else if !oplog.batch_importing && global_budget_flags() != 0 {
                        let _ = self.memory_stats_locked(&oplog, &state, 0);
                    }
                    let _ = state.store.take_created_journal();
                } else if !oplog.batch_importing && global_budget_flags() != 0 {
                    let _ = self.memory_stats_locked(&oplog, &state, 0);
                }
            }
        }

        if result.has_deps_before_shallow_root {
            if rollback_enabled {
                oplog.commit_import_rollback();
            }
            return Err(LoroError::ImportUpdatesThatDependsOnOutdatedVersion);
        }

        if rollback_enabled {
            oplog.commit_import_rollback();
        }
        Ok(result.status)
    }

    fn emit_events(&self) {
        // we should not hold the lock when emitting events
        let events = {
            let mut state = self.state.lock();
            state.take_events()
        };
        let mut panic = DeferredPanic::default();
        for event in events {
            panic.run(|| self.observer.emit(event));
        }
        panic.resume();
    }

    pub(crate) fn drop_pending_events(&self) -> Vec<DocDiff> {
        let mut state = self.state.lock();
        state.take_events()
    }

    /// Import the json schema updates.
    ///
    /// only supports backward compatibility but not forward compatibility.
    #[tracing::instrument(skip_all)]
    pub fn import_json_updates<T: TryInto<JsonSchema>>(&self, json: T) -> LoroResult<ImportStatus> {
        let json = json.try_into().map_err(|_| LoroError::InvalidJsonSchema)?;
        self.check_import_json_limit(&json)?;
        self.check_import_not_nested()?;
        self.with_barrier(|| {
            let result = self.import_changes_and_apply_delta_to_state_if_needed(
                |oplog| crate::encoding::json_schema::decode_json_changes(json, &oplog.arena),
                ImportedValues::JsonLossy,
                Default::default(),
            );
            self.emit_events();
            result
        })
    }

    pub fn export_json_updates(
        &self,
        start_vv: &VersionVector,
        end_vv: &VersionVector,
        with_peer_compression: bool,
    ) -> LoroResult<JsonSchema> {
        self.try_export_json_updates(start_vv, end_vv, with_peer_compression)
    }

    pub fn try_export_json_updates(
        &self,
        start_vv: &VersionVector,
        end_vv: &VersionVector,
        with_peer_compression: bool,
    ) -> LoroResult<JsonSchema> {
        self.with_barrier(|| {
            let oplog = self.oplog.lock();
            let mut start_vv = start_vv;
            let _temp: Option<VersionVector>;
            if !oplog.dag.shallow_since_vv().is_empty() {
                // Make sure that start_vv >= shallow_since_vv
                let mut include_all = true;
                for (peer, counter) in oplog.dag.shallow_since_vv().iter() {
                    if start_vv.get(peer).unwrap_or(&0) < counter {
                        include_all = false;
                        break;
                    }
                }
                if !include_all {
                    let mut vv = start_vv.clone();
                    for (&peer, &counter) in oplog.dag.shallow_since_vv().iter() {
                        vv.extend_to_include_end_id(ID::new(peer, counter));
                    }
                    _temp = Some(vv);
                    start_vv = _temp.as_ref().unwrap();
                }
            }

            crate::encoding::json_schema::try_export_json(
                &oplog,
                start_vv,
                end_vv,
                with_peer_compression,
            )
        })
    }

    pub fn export_json_in_id_span(&self, id_span: IdSpan) -> Vec<JsonChange> {
        let oplog = self.oplog.lock();
        let mut changes = export_json_in_id_span(&oplog, id_span);
        if let Some(uncommit) = oplog.get_uncommitted_change_in_span(id_span) {
            let change_json = encode_change(ChangeRef::from_change(&uncommit), &self.arena, None);
            changes.push(change_json);
        }
        changes
    }

    /// Get the version vector of the current OpLog
    #[inline]
    pub fn oplog_vv(&self) -> VersionVector {
        self.oplog.lock().vv().clone()
    }

    /// Get the version vector of the current [DocState].
    ///
    /// When the state frontiers are the op-log frontiers, this clones the cached
    /// op-log version vector and does not parse change blocks. Otherwise it
    /// walks the frontiers.
    ///
    /// # Panics
    ///
    /// Panics if the state's version cannot be resolved from the document history.
    /// Use [`Self::try_state_vv`] to handle unreadable history as an error.
    #[inline]
    pub fn state_vv(&self) -> VersionVector {
        self.try_state_vv()
            .expect("Cannot resolve the state version vector")
    }

    /// Get the version vector of the current state, returning a decode error
    /// when required history cannot be read.
    ///
    /// `Ok(None)` from the frontier walk (the frontiers are not in the dag) is
    /// [`LoroError::FrontiersNotFound`] for an id that can be named, not an empty
    /// version vector.
    pub fn try_state_vv(&self) -> LoroResult<VersionVector> {
        let oplog = self.oplog.lock();
        let frontiers = self.state.lock().frontiers.clone();
        if &frontiers == oplog.frontiers() {
            // `OpLog::vv` is the cached DAG version vector. It does not parse blocks.
            return Ok(oplog.vv().clone());
        }
        frontiers_to_vv_or_not_found(&oplog, &frontiers)
    }

    pub fn get_by_path(&self, path: &[Index]) -> Option<ValueOrHandler> {
        let value: LoroValue = self.state.lock().get_value_by_path(path)?;
        if let LoroValue::Container(c) = value {
            Some(ValueOrHandler::Handler(Handler::new_attached(
                c.clone(),
                self.clone(),
            )))
        } else {
            Some(ValueOrHandler::Value(value))
        }
    }

    /// Get the handler by the string path.
    pub fn get_by_str_path(&self, path: &str) -> Option<ValueOrHandler> {
        let path = str_to_path(path)?;
        self.get_by_path(&path)
    }

    pub fn get_uncommitted_ops_as_json(&self) -> Option<JsonSchema> {
        let arena = &self.arena;
        let txn = self.txn.lock();
        let txn = txn.as_ref()?;
        let ops_ = txn.local_ops();
        let new_id = ID {
            peer: *txn.peer(),
            counter: ops_.first()?.counter,
        };
        let change = ChangeRef {
            id: &new_id,
            deps: txn.frontiers(),
            timestamp: &txn
                .timestamp()
                .as_ref()
                .copied()
                .unwrap_or_else(|| self.oplog.lock().get_timestamp_for_next_txn()),
            commit_msg: txn.msg(),
            ops: ops_,
            lamport: txn.lamport(),
        };
        let json = encode_change_to_json(change, arena);
        Some(json)
    }

    #[inline]
    pub fn get_handler(&self, id: ContainerID) -> Option<Handler> {
        let idx = self.resolve_and_ensure_container(&id)?;
        Some(Handler::new_attached_with_idx(id, idx, self.clone()))
    }

    #[inline]
    fn resolve_and_ensure_container(&self, id: &ContainerID) -> Option<ContainerIdx> {
        // Mergeable roots stay lazily materialized: their existence is derived from the
        // parent map's child ref, and a store entry only appears once the child gets its
        // own ops (or via the shallow-export alive walk).
        if id.is_root() && !id.is_mergeable() {
            return Some(self.state.lock().ensure_container(id));
        }
        if !self.state.lock().does_container_exist(id) {
            return None;
        }
        Some(self.arena.register_container(id))
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn try_get_text<I: IntoContainerId>(&self, id: I) -> Option<TextHandler> {
        self.touch_access_tick();
        let id = id.into_container_id(&self.arena, ContainerType::Text);
        let idx = self.resolve_and_ensure_container(&id)?;
        Handler::new_attached_with_idx(id, idx, self.clone())
            .into_text()
            .ok()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_text<I: IntoContainerId>(&self, id: I) -> TextHandler {
        self.try_get_text(id)
            .expect("The container does not exist in the document. Use `try_get_text` or `get_container` to check for existence.")
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn try_get_list<I: IntoContainerId>(&self, id: I) -> Option<ListHandler> {
        self.touch_access_tick();
        let id = id.into_container_id(&self.arena, ContainerType::List);
        let idx = self.resolve_and_ensure_container(&id)?;
        Handler::new_attached_with_idx(id, idx, self.clone())
            .into_list()
            .ok()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_list<I: IntoContainerId>(&self, id: I) -> ListHandler {
        self.try_get_list(id)
            .expect("The container does not exist in the document. Use `try_get_list` or `get_container` to check for existence.")
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn try_get_movable_list<I: IntoContainerId>(&self, id: I) -> Option<MovableListHandler> {
        self.touch_access_tick();
        let id = id.into_container_id(&self.arena, ContainerType::MovableList);
        let idx = self.resolve_and_ensure_container(&id)?;
        Handler::new_attached_with_idx(id, idx, self.clone())
            .into_movable_list()
            .ok()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_movable_list<I: IntoContainerId>(&self, id: I) -> MovableListHandler {
        self.try_get_movable_list(id)
            .expect("The container does not exist in the document. Use `try_get_movable_list` or `get_container` to check for existence.")
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn try_get_map<I: IntoContainerId>(&self, id: I) -> Option<MapHandler> {
        self.touch_access_tick();
        let id = id.into_container_id(&self.arena, ContainerType::Map);
        let idx = self.resolve_and_ensure_container(&id)?;
        Handler::new_attached_with_idx(id, idx, self.clone())
            .into_map()
            .ok()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_map<I: IntoContainerId>(&self, id: I) -> MapHandler {
        self.try_get_map(id)
            .expect("The container does not exist in the document. Use `try_get_map` or `get_container` to check for existence.")
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn try_get_tree<I: IntoContainerId>(&self, id: I) -> Option<TreeHandler> {
        self.touch_access_tick();
        let id = id.into_container_id(&self.arena, ContainerType::Tree);
        let idx = self.resolve_and_ensure_container(&id)?;
        Handler::new_attached_with_idx(id, idx, self.clone())
            .into_tree()
            .ok()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_tree<I: IntoContainerId>(&self, id: I) -> TreeHandler {
        self.try_get_tree(id)
            .expect("The container does not exist in the document. Use `try_get_tree` or `get_container` to check for existence.")
    }

    #[cfg(feature = "counter")]
    pub fn try_get_counter<I: IntoContainerId>(
        &self,
        id: I,
    ) -> Option<crate::handler::counter::CounterHandler> {
        self.touch_access_tick();
        let id = id.into_container_id(&self.arena, ContainerType::Counter);
        let idx = self.resolve_and_ensure_container(&id)?;
        Handler::new_attached_with_idx(id, idx, self.clone())
            .into_counter()
            .ok()
    }

    #[cfg(feature = "counter")]
    pub fn get_counter<I: IntoContainerId>(
        &self,
        id: I,
    ) -> crate::handler::counter::CounterHandler {
        self.try_get_counter(id)
            .expect("The container does not exist in the document. Use `try_get_counter` or `get_container` to check for existence.")
    }

    #[must_use]
    pub fn has_container(&self, id: &ContainerID) -> bool {
        if id.is_root() && !id.is_mergeable() {
            return true;
        }

        let exist = self.state.lock().does_container_exist(id);
        exist
    }

    /// Undo the operations between the given id_span. It can be used even in a collaborative environment.
    ///
    /// This is an internal API. You should NOT use it directly.
    ///
    /// # Internal
    ///
    /// This method will use the diff calculator to calculate the diff required to time travel
    /// from the end of id_span to the beginning of the id_span. Then it will convert the diff to
    /// operations and apply them to the OpLog with a dep on the last id of the given id_span.
    ///
    /// This implementation is kinda slow, but it's simple and maintainable. We can optimize it
    /// further when it's needed. The time complexity is O(n + m), n is the ops in the id_span, m is the
    /// distance from id_span to the current latest version.
    ///
    /// Returns application errors after rolling back the inverse. Recreating
    /// an unknown container is rejected before application with an `ArgErr`.
    #[instrument(level = "info", skip_all)]
    pub fn undo_internal(
        &self,
        id_span: IdSpan,
        container_remap: &mut FxHashMap<ContainerID, ContainerID>,
        post_transform_base: Option<&DiffBatch>,
        before_diff: &mut dyn FnMut(&DiffBatch),
    ) -> LoroResult<CommitWhenDrop<'_>> {
        self.undo_internal_with(
            id_span,
            None,
            container_remap,
            post_transform_base,
            before_diff,
            &mut None,
            &mut Default::default(),
        )
    }

    /// [`Self::undo_internal`]. When the step is rejected because it would
    /// recreate a container of an unknown type, `rejected_step` is set to the
    /// step's own changes, which stay in the doc: the undo manager treats them
    /// like remote changes for the steps before it.
    /// `inverse`: the recorded inverse of `id_span`'s ops, if the undo manager has
    /// one (see `crate::undo::LocalInverse`). When `id_span` is one run, it
    /// replaces checking the document out at the span's last op and its deps.
    /// See context/undo-log.md.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn undo_internal_with(
        &self,
        id_span: IdSpan,
        inverse: Option<&crate::undo::LocalInverse>,
        container_remap: &mut FxHashMap<ContainerID, ContainerID>,
        post_transform_base: Option<&DiffBatch>,
        before_diff: &mut dyn FnMut(&DiffBatch),
        rejected_step: &mut Option<DiffBatch>,
        element_remap: &mut crate::undo::movable_list::ElementRemap,
    ) -> LoroResult<CommitWhenDrop<'_>> {
        let _exclusive = self.txn.lock_exclusive();
        if !self.can_edit() {
            return Err(LoroError::EditWhenDetached);
        }

        let (options, mut txn) = self.implicit_commit_then_stop();
        // Undo checks out earlier versions, which `checkout` refuses while a manual
        // transaction is open. Say so before anything changes.
        if self.state.lock().is_in_txn() {
            drop(txn);
            self.renew_txn_if_auto_commit(options);
            return Err(LoroError::DuplicatedTransactionError);
        }
        if !self.oplog().lock().vv().includes_id(id_span.id_last()) {
            self.renew_txn_if_auto_commit(options);
            return Err(LoroError::UndoInvalidIdSpan(id_span.id_last()));
        }

        let (was_recording, latest_frontiers) = {
            let mut state = self.state.lock();
            let was_recording = state.is_recording();
            state.stop_and_clear_recording();
            (was_recording, state.frontiers.clone())
        };

        let spans = self.oplog.lock().split_span_based_on_deps(id_span);
        // The recorded inverse replaces the checkouts only for exactly this
        // transition, and one run.
        let verify = crate::undo::verify_recorded_inverses();
        let mut recorded = inverse
            .and_then(|inv| inv.seal())
            .filter(|inv| spans.len() == 1 && inv.span == spans[0].0 && inv.deps == spans[0].1);
        // Every movable-list op of a recorded span is recorded, so its records
        // name the elements `scopes` would find by reading history.
        let scopes = match &recorded {
            Some(inv) if !verify => {
                let all = crate::undo::movable_list::scopes_from_records(&inv.movable);
                Ok((
                    vec![crate::undo::movable_list::scopes_from_records(&inv.movable)],
                    all,
                ))
            }
            _ => crate::undo::movable_list::scopes(self, &spans),
        };
        let (span_scopes, all_scopes) = match scopes {
            Ok(scopes) => scopes,
            Err(error) => {
                if was_recording {
                    self.state.lock().start_recording();
                }
                drop(txn);
                self.renew_txn_if_auto_commit(options);
                return Err(error);
            }
        };
        let inverse_lists = std::cell::RefCell::new(Vec::new());
        let revived_lists = std::cell::RefCell::new(Default::default());
        if verify {
            // What production takes from the records must be what history says
            // (a shallow root trimming the deleted elements' history aside).
            recorded = recorded.filter(|inv| {
                crate::undo::movable_list::records_match_scopes(&inv.movable, &all_scopes)
            });
        }
        let recorded = recorded.as_ref();
        let recorded_diff = std::cell::RefCell::new(recorded.map(|inv| inv.full_diff(self)));
        let recorded_used = std::cell::Cell::new(false);
        let checked_out = std::cell::Cell::new(false);
        let checkout_calc_diff = |from: &Frontiers, to: &Frontiers| {
            checked_out.set(true);
            self._checkout_without_emitting(from, false, false).unwrap();
            let inverse_index = spans
                .iter()
                .position(|(span, deps)| Frontiers::from_id(span.id_last()) == *from && deps == to);
            let after_lists: Vec<_> = inverse_index.map_or_else(Vec::new, |i| {
                span_scopes[i]
                    .iter()
                    .map(|(cid, scope)| {
                        (
                            cid.clone(),
                            crate::undo::movable_list::Snapshot::capture_span(
                                self,
                                cid,
                                scope,
                                &all_scopes[cid],
                                false,
                            ),
                        )
                    })
                    .collect()
            });
            {
                let mut state = self.state.lock();
                state.start_recording();
                state.set_record_changes_only(true);
            }
            self._checkout_without_emitting(to, false, false).unwrap();
            for (cid, after) in after_lists {
                let i = inverse_index.unwrap();
                let before = crate::undo::movable_list::Snapshot::capture_span(
                    self,
                    &cid,
                    &span_scopes[i][&cid],
                    &all_scopes[&cid],
                    true,
                );
                inverse_lists
                    .borrow_mut()
                    .push(crate::undo::movable_list::Inverse {
                        span_index: inverse_index.unwrap(),
                        cid,
                        before,
                        after,
                        before_frontiers: to.clone(),
                    });
            }
            let mut state = self.state.lock();
            let e = state.take_events();
            state.set_record_changes_only(false);
            state.stop_and_clear_recording();
            // Applied to this doc, which kept the state of re-activated mergeable children.
            let diff = DiffBatch::from_changes(e);
            drop(state);
            if let Some(i) = inverse_index {
                crate::undo::movable_list::capture_revivals(
                    self,
                    &diff,
                    i,
                    &mut revived_lists.borrow_mut(),
                );
            }
            diff
        };
        let calc_diff = |from: &Frontiers, to: &Frontiers| {
            if let Some(inv) = recorded {
                if *from == Frontiers::from_id(inv.span.id_last()) && *to == inv.deps {
                    if let Some(diff) = recorded_diff.borrow_mut().take() {
                        if verify {
                            let from_checkout = checkout_calc_diff(from, to);
                            crate::undo::assert_same_undo_diff(&from_checkout, &diff);
                        }
                        #[cfg(test)]
                        crate::undo::RECORDED_INVERSES_USED.with(|n| n.set(n.get() + 1));
                        recorded_used.set(true);
                        return diff;
                    }
                }
            }
            checkout_calc_diff(from, to)
        };
        // `before_diff` rebases the undo manager's other steps over this one,
        // so it only runs once the step is accepted
        let mut last_event_a = None;
        let mut diff = crate::undo::undo(
            spans.clone(),
            match post_transform_base {
                Some(d) => Either::Right(d),
                None => Either::Left(&latest_frontiers),
            },
            calc_diff,
            &mut |d| last_event_a = Some(d.clone()),
        );

        if checked_out.get() {
            self._checkout_without_emitting(&latest_frontiers, false, false)?;
            self.set_detached(false);
        }
        let mut inverse_lists = inverse_lists.take();
        if let Some(inv) = recorded.filter(|_| recorded_used.get()) {
            let mut from_records: Vec<_> = inv
                .movable
                .iter()
                .map(|r| r.to_inverse(&inv.deps))
                .collect();
            if verify && !from_records.is_empty() {
                // `inverse_lists` holds what the checkout captured.
                for lists in [&mut inverse_lists, &mut from_records] {
                    crate::undo::movable_list::complete_anchors(
                        self,
                        lists,
                        &latest_frontiers,
                        &all_scopes,
                        element_remap,
                        container_remap,
                    )?;
                }
                crate::undo::movable_list::assert_same_inverses(
                    &inverse_lists,
                    &from_records,
                    self.is_shallow(),
                );
            }
            inverse_lists = from_records;
        }
        crate::undo::movable_list::complete_anchors(
            self,
            &mut inverse_lists,
            &latest_frontiers,
            &all_scopes,
            element_remap,
            container_remap,
        )?;
        let mut movable_plans = crate::undo::movable_list::prepare(
            self,
            &mut diff,
            inverse_lists,
            revived_lists.take(),
            container_remap,
            element_remap,
        );
        self.order_mergeable_removals(&mut diff, container_remap);
        // A step that would recreate an unknown container is rejected as a
        // whole, before anything is applied
        let rejected = self
            .check_apply_diff_creates_no_unknown_container(
                &diff,
                None,
                container_remap,
                true,
                &movable_plans,
            )
            .err();
        if rejected.is_some() {
            let mut changes = DiffBatch::default();
            for (span, deps) in spans.iter() {
                changes.compose(&calc_diff(deps, &span.id_last().into()));
            }
            self._checkout_without_emitting(&latest_frontiers, false, false)?;
            self.set_detached(false);
            *rejected_step = Some(changes);
        }

        if was_recording {
            self.state.lock().start_recording();
        }
        // Native identity edits must use the same transaction lock that
        // protected capture/preparation. Releasing it here lets a waiting
        // writer delete elements or anchors before the plan uses them.
        let remap_before = container_remap.clone();
        let elements_before = element_remap.clone();
        let mut undo_scope = None;
        let native_result: LoroResult<()> = (|| {
            if rejected.is_some() {
                return Ok(());
            }
            *txn = Some(self.txn_under_lock()?);
            let transaction = txn
                .as_mut()
                .ok_or_else(|| LoroError::TransactionError("undo transaction is missing".into()))?;
            // Keep the capture/preparation guard held while marking the scope
            // and applying native plans. Ordinary handlers later reuse this
            // same transaction and scope after the guard is released.
            undo_scope = Some(
                self.begin_apply_diff_scope_with_txn(transaction)
                    .expect("undo must own its rollback scope"),
            );
            let ids: Vec<_> = diff
                .order
                .iter()
                .filter(|id| movable_plans.get(*id).is_some_and(|p| p.is_native()))
                .cloned()
                .collect();
            for cid in ids {
                let Some(plan) = movable_plans.remove(&cid) else {
                    continue;
                };
                let (id, _) = resolve_container_remap(cid.clone(), container_remap);
                let list = self
                    .get_handler(id)
                    .and_then(|h| h.into_movable_list().ok())
                    .ok_or_else(|| {
                        LoroError::NotFoundError("MovableList undo container disappeared".into())
                    })?;
                plan.apply_with_txn(&list, transaction, container_remap, element_remap)?;
                #[cfg(test)]
                if RUN_APPLY_DIFF_HOOK_AFTER_NATIVE_PLAN.with(|flag| flag.replace(false)) {
                    after_apply_diff_handler()?;
                }
                diff.cid_to_events.remove(&cid);
            }
            diff.order.retain(|id| diff.cid_to_events.contains_key(id));
            Ok(())
        })();
        drop(txn);
        self.start_auto_commit();
        let applied = native_result.and_then(|()| {
            if rejected.is_none() {
                self.apply_diff_with_movable_undo(
                    diff,
                    container_remap,
                    true,
                    false,
                    &mut movable_plans,
                    element_remap,
                )
            } else {
                Ok(())
            }
        });
        if let Some(scope) = undo_scope {
            self.finish_undo_rollback(scope, applied.is_ok());
        }

        if let Some(options) = options {
            self.set_next_commit_options(options);
        }
        if let Some(e) = rejected {
            return Err(e);
        }
        if let Err(e) = applied {
            *container_remap = remap_before;
            *element_remap = elements_before;
            return Err(e);
        }
        if let Some(event_a) = &last_event_a {
            before_diff(event_a);
        }
        Ok(CommitWhenDrop {
            doc: self,
            default_options: CommitOptions::new().origin("undo"),
        })
    }

    /// Generate a series of local operations that can revert the current doc to the target
    /// version.
    ///
    /// Internally, it will calculate the diff between the current state and the target state,
    /// and apply the diff to the current state.
    pub fn revert_to(&self, target: &Frontiers) -> LoroResult<()> {
        // TODO: test when the doc is readonly
        // TODO: test when the doc is detached but enabled editing
        let f = self.state_frontiers();
        let diff = self.diff_events(&f, target, true)?;
        // This doc kept the state of the mergeable children the target re-activates, so apply
        // their actual changes rather than the full states `diff` reports.
        self.apply_diff_all_or_nothing(
            DiffBatch::from_changes(diff),
            &mut Default::default(),
            false,
            false,
        )
    }

    /// Calculate the diff between two versions so that apply diff on a will make the state same as b.
    ///
    /// NOTE: This method will make the doc enter the **detached mode**.
    // FIXME: This method needs testing (no event should be emitted during processing this)
    pub fn diff(&self, a: &Frontiers, b: &Frontiers) -> LoroResult<DiffBatch> {
        self.diff_events(a, b, false).map(|e| DiffBatch {
            full_state: true,
            ..DiffBatch::new(e)
        })
    }

    /// With `changes_only`, only [`DiffBatch::from_changes`] of the result is meaningful.
    // Both callers return a `Result`, so a history that cannot be parsed is rejected here.
    fn diff_events(
        &self,
        a: &Frontiers,
        b: &Frontiers,
        changes_only: bool,
    ) -> LoroResult<Vec<DocDiff>> {
        {
            // Check whether a and b are valid before checkout so this returns a normal error
            // instead of panicking on shallow docs.
            let oplog = self.oplog.lock();
            oplog.check_history_parsable()?;
            let validate_frontiers = |frontiers: &Frontiers| -> LoroResult<()> {
                for id in frontiers.iter() {
                    if !oplog.dag.contains(id) {
                        return Err(LoroError::FrontiersNotFound(id));
                    }
                }

                if oplog.dag.is_before_shallow_root(frontiers) {
                    return Err(LoroError::SwitchToVersionBeforeShallowRoot);
                }

                Ok(())
            };

            validate_frontiers(a)?;
            validate_frontiers(b)?;
        }

        let (options, txn) = self.implicit_commit_then_stop();
        let was_detached = self.is_detached();
        let old_frontiers = self.state_frontiers();
        let was_recording = {
            let mut state = self.state.lock();
            let is_recording = state.is_recording();
            state.stop_and_clear_recording();
            is_recording
        };
        let result = (|| {
            self._checkout_without_emitting(a, true, false)?;
            {
                let mut state = self.state.lock();
                state.start_recording();
                state.set_record_changes_only(changes_only);
            }
            let checkout = self._checkout_without_emitting(b, true, false);
            let mut state = self.state.lock();
            let e = checkout.map(|()| state.take_events());
            state.set_record_changes_only(false);
            let e = e?;
            state.stop_and_clear_recording();
            Ok::<_, LoroError>(e)
        })();

        // Always restore state regardless of whether diff calculation succeeded
        self._checkout_without_emitting(&old_frontiers, false, false)
            .unwrap();
        drop(txn);
        if !was_detached {
            self.set_detached(false);
            self.renew_txn_if_auto_commit(options);
        }
        if was_recording {
            self.state.lock().start_recording();
        }
        result
    }

    /// Apply a diff to the current state.
    #[inline(always)]
    ///
    /// A mergeable child that the batch re-activates is aligned with this doc's hidden state
    /// only if `diff.full_state` is set (batches from [`LoroDoc::diff`]); otherwise its entry
    /// is applied as an increment. See [`DiffBatch::full_state`].
    ///
    /// All or nothing: when it returns `Err`, the doc, its history and its subscribers are as
    /// before the call, and uncommitted edits made before it are kept. See
    /// `context/apply-diff-atomicity.md`.
    pub fn apply_diff(&self, diff: DiffBatch) -> LoroResult<()> {
        let align = diff.full_state;
        self.apply_diff_all_or_nothing(diff, &mut Default::default(), true, align)
    }

    /// [`Self::_apply_diff`], rolled back when it fails (loro-dev/loro#1154).
    /// Undo uses the same scope with its identity plans; see `context/undo-atomicity.md`.
    pub(crate) fn apply_diff_all_or_nothing(
        &self,
        diff: DiffBatch,
        container_remap: &mut FxHashMap<ContainerID, ContainerID>,
        skip_unreachable: bool,
        align_revived_mergeable: bool,
    ) -> LoroResult<()> {
        if !self.can_edit() {
            return Err(LoroError::EditWhenDetached);
        }

        // Rollback checks the state out through the current frontiers. If that
        // history cannot be read, refuse before the batch mutates anything.
        {
            let oplog = self.oplog.lock();
            let frontiers = self.state.lock().frontiers.clone();
            frontiers_to_vv_or_not_found(&oplog, &frontiers)?;
        }

        self.touch_access_tick();
        let linear_before = wasm_linear_memory_bytes();
        let scope = self.begin_apply_diff_scope();
        let mut result = self._apply_diff(
            diff,
            container_remap,
            skip_unreachable,
            align_revived_mergeable,
        );
        if result.is_ok() {
            let guard = self.txn.lock();
            if let Some(txn) = guard.as_ref() {
                if txn.doc_budget_limit.is_some()
                    || (global_budget_flags() & BUDGET_FLAG_MAX_DOC != 0
                        && txn.budget_base_scope().is_some())
                {
                    self.enforce_memory_watermarks_with_txn_bytes(
                        txn.estimated_uncommitted_bytes(),
                        false,
                    );
                    if let Err(err) = self.check_transaction_budget(txn, Some(linear_before)) {
                        result = Err(err);
                    }
                }
            }
        }

        if let Some(scope) = scope {
            if result.is_ok() {
                self.end_apply_diff_scope(scope);
            } else {
                self.rollback_apply_diff_scope(scope)?;
            }
        }
        if global_budget_flags() != 0 {
            self.refresh_cached_doc_bytes_unlocked();
        }
        result
    }

    /// Undo owns the writer gate and a fresh transaction. Losing its rollback
    /// scope or failing to restore state is an invariant failure, not a normal
    /// error that callers could safely retry on a partly changed document.
    fn finish_undo_rollback(&self, scope: u64, succeeded: bool) {
        if succeeded {
            self.end_apply_diff_scope(scope);
        } else {
            assert!(
                self.txn
                    .lock()
                    .as_ref()
                    .is_some_and(|txn| txn.rollback_scope_is_intact(scope)),
                "undo lost its exclusive rollback scope"
            );
            self.rollback_apply_diff_scope(scope)
                .expect("undo rollback must restore the document");
        }
    }

    /// Marks where the batch starts in the transaction it will run in.
    ///
    /// Returns `None`, and the batch runs without a rollback, when there is no transaction
    /// to run it in (the batch then fails before it applies anything) or another thread's
    /// batch runs in it.
    fn begin_apply_diff_scope(&self) -> Option<u64> {
        let mut guard = self.txn.lock();
        if guard.is_none() && !cfg!(target_arch = "wasm32") && self.can_edit() {
            // Like the handlers' `with_txn`, which the batch's first op would call
            drop(guard);
            self.start_auto_commit();
            guard = self.txn.lock();
        }
        self.begin_apply_diff_scope_with_txn(guard.as_mut()?)
    }

    fn next_rollback_scope_id() -> u64 {
        static NEXT_SCOPE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        NEXT_SCOPE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Also used by undo while it holds the preparation transaction guard.
    fn begin_apply_diff_scope_with_txn(&self, txn: &mut Transaction) -> Option<u64> {
        if txn.has_rollback_scope() {
            return None;
        }
        let id = Self::next_rollback_scope_id();
        txn.begin_rollback_scope(id);
        self.state.lock().begin_local_rollback(id);
        Some(id)
    }

    fn end_apply_diff_scope(&self, scope: u64) {
        let mut guard = self.txn.lock();
        if let Some(txn) = guard.as_mut() {
            txn.end_rollback_scope(scope);
        }
        self.state.lock().end_local_rollback(scope);
    }

    pub(crate) fn rollback_transaction_scope(
        &self,
        txn: &mut Transaction,
        scope: u64,
    ) -> LoroResult<()> {
        if !txn.rollback_scope_is_intact(scope) {
            txn.end_rollback_scope(scope);
            self.state.lock().end_local_rollback(scope);
            return Ok(());
        }
        self.rollback_transaction_scope_inner(txn, scope)
    }

    /// Undo the ops of a failed batch, keeping the ops the transaction held before it.
    /// Emits nothing.
    ///
    /// The transaction's ops are inserted into the op log as a change, under an import
    /// rollback scope, so that the diff calculator can compute the checkout from the batch's
    /// last op back to the version before the batch; then the scope is rolled back and the
    /// ops before the batch are put back into the DAG's version as pending local ops.
    fn rollback_apply_diff_scope(&self, scope: u64) -> LoroResult<()> {
        let mut guard = self.txn.lock();
        let Some(txn) = guard
            .as_mut()
            .filter(|txn| txn.rollback_scope_is_intact(scope))
        else {
            // Another thread committed the transaction or applied ops in it while the batch
            // ran. Undoing it would drop their edits, so the batch stays partly applied.
            warn!("apply_diff failed while another thread edited the doc; not rolled back");
            if let Some(txn) = guard.as_mut() {
                txn.end_rollback_scope(scope);
            }
            self.state.lock().end_local_rollback(scope);
            return Ok(());
        };
        self.rollback_transaction_scope_inner(txn, scope)
    }

    fn rollback_transaction_scope_inner(
        &self,
        txn: &mut Transaction,
        scope: u64,
    ) -> LoroResult<()> {
        let batch = txn.roll_back_scope(scope);
        let txn_now_empty = txn.is_empty();
        let mut oplog = self.oplog.lock();
        let mut state = self.state.lock();
        if batch.batch_start < batch.batch_end {
            let peer = batch.peer;
            let txn_ops_before = (batch.batch_start - batch.txn_start_counter) as usize;
            let before_batch = if txn_ops_before == 0 {
                batch.deps.clone()
            } else {
                Frontiers::from_id(ID::new(peer, batch.batch_start - 1))
            };
            #[cfg(debug_assertions)]
            {
                // Only the txn's ops changed the DAG's version since it began: anything else
                // (an import, a checkout) commits the txn first.
                let mut expected = batch.dag_frontiers_at_start.clone();
                expected.update_frontiers_on_new_change(
                    ID::new(peer, batch.batch_end - 1),
                    &batch.deps,
                );
                assert_eq!(&expected, oplog.dag.frontiers());
                assert_eq!(oplog.dag.vv().get(&peer), Some(&batch.batch_end));
            }
            oplog.dag.discard_pending_txn(
                batch.dag_frontiers_at_start,
                peer,
                batch.txn_start_counter,
            );
            oplog.begin_import_rollback();
            oplog.insert_new_change(
                Change {
                    id: ID::new(peer, batch.txn_start_counter),
                    lamport: batch.txn_start_lamport,
                    deps: batch.deps.clone(),
                    timestamp: batch.timestamp,
                    commit_msg: None,
                    ops: batch.ops,
                },
                false,
            );

            let batch_last = state.frontiers.clone();
            debug_assert_eq!(
                batch_last,
                Frontiers::from_id(ID::new(peer, batch.batch_end - 1))
            );
            let undone = (|| -> LoroResult<()> {
                let from = frontiers_to_vv_or_not_found(&oplog, &batch_last)?;
                let to = frontiers_to_vv_or_not_found(&oplog, &before_batch)?;
                let (diff, diff_mode) = DiffCalculator::new(false).try_calc_diff_internal(
                    &oplog,
                    &from,
                    &batch_last,
                    &to,
                    &before_batch,
                    None,
                )?;
                // `DocState::apply_diff` refuses to run inside a transaction, and records nothing
                // without its event recorder.
                state.abort_txn();
                let recorder = state.take_event_recorder();
                let applied = state.apply_diff(
                    InternalDocDiff {
                        origin: Default::default(),
                        by: EventTriggerKind::Checkout,
                        diff: Cow::Owned(diff),
                        new_version: Cow::Owned(before_batch.clone()),
                        local_inverse: None,
                    },
                    diff_mode,
                );
                state.restore_event_recorder(recorder);
                state.resume_txn();
                applied
            })();
            // The temporary change must not stay in the op log if the checkout cannot be read.
            oplog.rollback_import_keeping_arena(&state);
            oplog.free_history_cache();
            if txn_ops_before > 0 {
                oplog.dag.update_version_on_new_local_op(
                    &batch.deps,
                    ID::new(peer, batch.txn_start_counter),
                    batch.txn_start_lamport,
                    txn_ops_before,
                );
                oplog.refresh_visible_op_count();
            }
            undone?;
        }
        state.finish_local_rollback(scope, batch.peer, batch.batch_start);
        if txn_now_empty {
            state.clear_changed_idx_in_txn();
        }
        Ok(())
    }

    /// Apply a diff to the current state.
    ///
    /// Regular containers are recreated with new [ContainerID]s. Reusing their
    /// original IDs could break several internal invariants:
    ///
    /// 1. Each container should appear only once in the document. Allowing containers with the same ID
    ///    would result in multiple instances of the same container in the document.
    /// 2. Unreachable containers should be removable from the state when necessary.
    ///
    /// However, the diff may contain operations that depend on container IDs.
    /// Therefore, users need to provide a `container_remap` to record and retrieve the container ID remapping.
    /// This remapping applies to the diff/undo workflow, not to imported operations.
    /// Mergeable children instead restore a deterministic parent/key/type marker;
    /// their identity is preserved only when the parent identity is preserved.
    ///
    /// With `align_revived_mergeable` (a [`DiffBatch::full_state`] batch), a mergeable child the
    /// batch re-activates carries its full state (no entry means empty), which is aligned with
    /// whatever hidden state this doc has at its deterministic cid. Otherwise entries are
    /// increments; revert and undo pass batches of actual changes ([`DiffBatch::from_changes`]).
    /// See context/mergeable-containers.md.
    pub(crate) fn _apply_diff(
        &self,
        diff: DiffBatch,
        container_remap: &mut FxHashMap<ContainerID, ContainerID>,
        skip_unreachable: bool,
        align_revived_mergeable: bool,
    ) -> LoroResult<()> {
        self.apply_diff_with_movable_undo(
            diff,
            container_remap,
            skip_unreachable,
            align_revived_mergeable,
            &mut Default::default(),
            &mut Default::default(),
        )
    }

    fn apply_diff_with_movable_undo(
        &self,
        mut diff: DiffBatch,
        container_remap: &mut FxHashMap<ContainerID, ContainerID>,
        skip_unreachable: bool,
        align_revived_mergeable: bool,
        movable_plans: &mut FxHashMap<ContainerID, crate::undo::movable_list::Plan>,
        element_remap: &mut crate::undo::movable_list::ElementRemap,
    ) -> LoroResult<()> {
        if !self.can_edit() {
            return Err(LoroError::EditWhenDetached);
        }

        self.order_mergeable_removals(&mut diff, container_remap);

        // When the batch holds unknown containers, its full states are aligned
        // before anything is applied, so the check sees what is applied; the
        // loop below then applies the planned edits. Otherwise the loop aligns
        // each full state itself.
        let mut plan =
            if align_revived_mergeable && diff.iter().any(|(_, d)| diff_has_unknown_value(d)) {
                self.plan_full_state_batch(&diff, container_remap)
            } else {
                FullStatePlan::default()
            };
        // Reject unsupported container recreation before touching the doc.
        let predicted_skips = self.check_apply_diff_creates_no_unknown_container(
            &diff,
            align_revived_mergeable.then_some(&plan),
            container_remap,
            skip_unreachable,
            movable_plans,
        )?;

        let mut ans: LoroResult<()> = Ok(());
        let mut missing_containers: Vec<ContainerID> = Vec::new();
        // Containers whose diff in this batch is a full state while they keep an id that may
        // already hold state: re-activated mergeable children and, transitively, the children
        // of aligned containers that were kept.
        let mut full_state_targets: FxHashSet<ContainerID> = FxHashSet::default();
        let mut applied: FxHashSet<ContainerID> = FxHashSet::default();
        // The metadata maps of tree nodes this batch moved back from the deleted
        // tree, and the kept children found aligning them. Their entries are full
        // states in any batch, since the batch made the node with a `Create`
        // (see context/tree-events.md, "Undo brings deleted nodes back").
        let mut revived_full_state: FxHashSet<ContainerID> = FxHashSet::default();
        for (mut id, diff) in diff.into_iter() {
            let batch_id = id.clone();
            let mut remapped = false;
            while let Some(rid) = container_remap.get(&id) {
                remapped = true;
                id = rid.clone();
            }

            let from_revival = revived_full_state.remove(&id);
            let is_full_state =
                from_revival || (align_revived_mergeable && full_state_targets.remove(&id));
            applied.insert(id.clone());

            if matches!(&id, ContainerID::Normal { .. }) && self.arena.id_to_idx(&id).is_none() {
                // Not in arena does not imply non-existent; consult state/kv and register lazily
                let exists = self.state.lock().does_container_exist(&id);
                if !exists {
                    missing_containers.push(id);
                    continue;
                }
                // Ensure registration so handlers can be created
                self.state.lock().ensure_container(&id);
            }

            // A revived node's metadata can be deleted again later in the same
            // batch, so it is checked like any other container.
            if skip_unreachable && (!is_full_state || from_revival) {
                let mut state = self.state.lock();
                // A remapped target is normally recreated by this batch and is
                // applied even before it is attached. An earlier undo may have
                // recreated it, though, and it may have been deleted since:
                // writing to it would fail with `ContainerDeleted`, so skip it
                // like any other unreachable container.
                let skip = if remapped {
                    self.arena
                        .id_to_idx(&id)
                        .is_some_and(|idx| state.is_deleted(idx))
                } else {
                    !state.get_reachable(&id)
                };
                if skip {
                    continue;
                }
            }
            debug_assert!(
                !predicted_skips.contains(&batch_id),
                "{batch_id} holds an unknown container and was predicted to be skipped"
            );

            let Some(h) = self.get_handler(id.clone()) else {
                return Err(LoroError::ContainersNotFound {
                    containers: Box::new(vec![id]),
                });
            };
            let planned = plan.aligned.remove(&batch_id);
            let diff = if is_full_state {
                match planned {
                    // The plan aligned this entry against the same container
                    Some(p) if p.target == id => {
                        if from_revival {
                            revived_full_state.extend(p.kept);
                        } else {
                            full_state_targets.extend(p.kept);
                        }
                        match p.edit {
                            Some(edit) => edit,
                            None => continue,
                        }
                    }
                    _ => {
                        // Not planned (the batch holds no unknown container),
                        // or a mergeable child of a container this batch
                        // recreated: new and empty, which the plan checked as such
                        debug_assert!(
                            from_revival
                                || plan.fresh.contains(&batch_id)
                                || !diff_has_unknown_value(&diff),
                            "{batch_id} holds an unknown container and was not planned"
                        );
                        let current = self.state.lock().container_full_diff(h.container_idx());
                        let kept = if from_revival {
                            &mut revived_full_state
                        } else {
                            &mut full_state_targets
                        };
                        match crate::handler::align_full_state(&h, &diff, current, kept) {
                            Some(diff) => diff,
                            None => continue,
                        }
                    }
                }
            } else {
                debug_assert!(
                    planned.is_none() || !diff_has_unknown_value(&diff),
                    "{batch_id} holds an unknown container and was planned as a full state"
                );
                diff
            };
            let result = if let Some(movable) = movable_plans.remove(&batch_id) {
                let Handler::MovableList(list) = &h else {
                    unreachable!()
                };
                movable.apply(list, diff, container_remap, element_remap)
            } else {
                h.apply_diff(
                    diff,
                    container_remap,
                    &mut full_state_targets,
                    &mut revived_full_state,
                )
            };
            // Fault injection after real operations exercises rollback.
            #[cfg(test)]
            let result = result.and_then(|()| after_apply_diff_handler());
            if let Err(e) = result {
                debug_assert!(
                    !crate::handler::is_unknown_container_creation_err(&e),
                    "the unknown container check missed {e:?} in {batch_id}"
                );
                ans = Err(e);
            }
        }

        // A revived node's metadata, or a child of it, with no entry in the batch is empty.
        for id in revived_full_state {
            if applied.contains(&id) {
                continue;
            }
            let Some(h) = self.get_handler(id.clone()) else {
                continue;
            };
            let mut state = self.state.lock();
            // Deleted again later in the batch: nothing to clear.
            let is_empty =
                !state.get_reachable(&id) || state.is_container_state_empty(h.container_idx());
            drop(state);
            if !is_empty {
                if let Err(e) = h.clear() {
                    ans = Err(e);
                }
            }
        }

        if align_revived_mergeable {
            // A full-state container without an entry in the batch is empty.
            for id in full_state_targets {
                if applied.contains(&id) {
                    continue;
                }
                let Some(h) = self.get_handler(id) else {
                    continue;
                };
                let is_empty = self
                    .state
                    .lock()
                    .is_container_state_empty(h.container_idx());
                if !is_empty {
                    if let Err(e) = h.clear() {
                        ans = Err(e);
                    }
                }
            }
        }

        if !missing_containers.is_empty() {
            return Err(LoroError::ContainersNotFound {
                containers: Box::new(missing_containers),
            });
        }

        ans
    }

    /// Apply edits to a visible mergeable subtree before a map diff hides it.
    /// Its deterministic id can be re-ensured later, so skipping those edits
    /// would retain content that undo was meant to remove (#1255). Ordering
    /// also keeps ordinary descendants writable without bypassing the
    /// transaction's deleted-container check. Already hidden subtrees retain
    /// the usual skip behavior.
    fn order_mergeable_removals(
        &self,
        diff: &mut DiffBatch,
        container_remap: &FxHashMap<ContainerID, ContainerID>,
    ) {
        if diff.order.len() <= 1 {
            return;
        }
        let mut removals = FxHashMap::default();
        for (i, (id, edit)) in diff.iter().enumerate() {
            let crate::event::Diff::Map(map) = edit else {
                continue;
            };
            let (target, _) = resolve_container_remap(id.clone(), container_remap);
            // Like the unknown-container preflight, inspect existing state
            // without materializing a new root before the batch is accepted.
            if (target.is_root()
                && !target.is_mergeable()
                && self.arena.id_to_idx(&target).is_none())
                || !self.has_container(&target)
            {
                continue;
            }
            let Handler::Map(handler) = Handler::new_attached(target.clone(), self.clone()) else {
                continue;
            };
            for (key, value) in &map.updated {
                let Some(LoroValue::Container(old)) = handler.get(key) else {
                    continue;
                };
                if !old.is_mergeable() {
                    continue;
                }
                let new = value
                    .value
                    .as_ref()
                    .and_then(diff_value_container_id)
                    .map(|cid| {
                        if cid.is_mergeable() {
                            ContainerID::new_mergeable(&target, key, cid.container_type())
                        } else {
                            resolve_container_remap(cid, container_remap).0
                        }
                    });
                if new.as_ref() != Some(&old) {
                    removals.insert(old, i);
                }
            }
        }
        if removals.is_empty() {
            return;
        }

        // Stable topological order: each reachable descendant must precede
        // the map entry removing its mergeable ancestor. All dependencies
        // follow the arena ancestry, so they cannot form a cycle.
        let mut dependents = vec![Vec::new(); diff.order.len()];
        let mut pending = vec![0usize; diff.order.len()];
        for (i, id) in diff.order.iter().enumerate() {
            let (target, _) = resolve_container_remap(id.clone(), container_remap);
            if !self.state.lock().get_reachable(&target) {
                continue;
            }
            for ancestor in self.arena_ancestors_and_self(&target) {
                if let Some(&parent) = removals.get(&ancestor) {
                    dependents[i].push(parent);
                    pending[parent] += 1;
                }
            }
        }
        let mut ready: std::collections::BinaryHeap<_> = pending
            .iter()
            .enumerate()
            .filter(|(_, count)| **count == 0)
            .map(|(i, _)| std::cmp::Reverse(i))
            .collect();
        let mut order = Vec::with_capacity(diff.order.len());
        while let Some(std::cmp::Reverse(i)) = ready.pop() {
            order.push(diff.order[i].clone());
            for &parent in &dependents[i] {
                pending[parent] -= 1;
                if pending[parent] == 0 {
                    ready.push(std::cmp::Reverse(parent));
                }
            }
        }
        assert_eq!(
            order.len(),
            diff.order.len(),
            "cyclic mergeable removal order"
        );
        diff.order = order;
    }

    /// How `_apply_diff` treats a full-state batch holding unknown containers,
    /// planned before anything is applied: which entries it aligns with the
    /// state this doc kept, and the edits they turn into. The loop applies
    /// these edits, so alignment is computed once, and the unknown container
    /// check sees what is applied.
    ///
    /// The loop still keeps its own `full_state_targets`, because it follows
    /// `container_remap` as it grows. It uses a planned edit only when it
    /// aligns the same container as the plan did.
    fn plan_full_state_batch(
        &self,
        diff: &DiffBatch,
        container_remap: &FxHashMap<ContainerID, ContainerID>,
    ) -> FullStatePlan {
        let mut plan = FullStatePlan::default();
        // The loop's `full_state_targets`
        let mut targets: FxHashSet<ContainerID> = FxHashSet::default();
        // Containers the loop may recreate under a new id: container values of
        // Map/List diffs and the metas of created or moved tree nodes. Errs
        // towards "recreated", which only makes the check stricter.
        let mut recreated: FxHashSet<ContainerID> = FxHashSet::default();
        // Mergeable children of recreated containers: the loop aligns them
        // against a new, empty container
        let mut fresh_targets: FxHashSet<ContainerID> = FxHashSet::default();
        for (id, diff) in diff.iter() {
            let (target, _) = resolve_container_remap(id.clone(), container_remap);
            match diff {
                crate::event::Diff::Map(map) => recreated.extend(
                    map.updated
                        .values()
                        .filter_map(|v| v.value.as_ref().and_then(diff_value_container_id))
                        .filter(|c| !c.is_mergeable()),
                ),
                crate::event::Diff::List(delta) => {
                    recreated.extend(inserted_container_ids(delta));
                }
                crate::event::Diff::Tree(tree) => recreated.extend(
                    tree.diff
                        .iter()
                        .filter(|item| {
                            !matches!(item.action, crate::delta::TreeExternalDiff::Delete { .. })
                        })
                        .map(|item| item.target.associated_meta_container()),
                ),
                _ => {}
            }

            let effective = if fresh_targets.remove(&target) {
                targets.remove(&target);
                plan.always_applied.insert(target.clone());
                plan.fresh.insert(id.clone());
                recreated.insert(target.clone());
                Some(diff)
            } else if targets.remove(&target) {
                plan.always_applied.insert(target.clone());
                let Some(h) = self.get_handler(target.clone()) else {
                    // `_apply_diff` stops here with `ContainersNotFound`
                    continue;
                };
                let current = self.state.lock().container_full_diff(h.container_idx());
                let mut kept = FxHashSet::default();
                let edit = crate::handler::align_full_state(&h, diff, current, &mut kept);
                targets.extend(kept.iter().cloned());
                let entry = plan.aligned.entry(id.clone()).or_insert(PlannedAlignment {
                    target: target.clone(),
                    edit,
                    kept,
                });
                entry.edit.as_ref()
            } else if recreated.contains(&target)
                || container_remap.contains_key(id)
                || !self.has_container(&target)
                || self.state.lock().get_reachable(&target)
            {
                Some(diff)
            } else {
                // The loop skips an entry whose target is unreachable before the
                // batch, unless an earlier entry revived it, which would have
                // made it a target above. So its marker writes don't count (the
                // batch order is up to the caller).
                None
            };

            // A mergeable marker written by a Map diff makes its child a target
            if let Some(crate::event::Diff::Map(map)) = effective {
                for (key, v) in map.updated.iter() {
                    if let Some(c) = v.value.as_ref().and_then(diff_value_container_id) {
                        if c.is_mergeable() {
                            let child =
                                ContainerID::new_mergeable(&target, key, c.container_type());
                            if recreated.contains(&target) {
                                fresh_targets.insert(child);
                            } else {
                                targets.insert(child);
                            }
                        }
                    }
                }
            }
        }
        plan
    }

    /// `_apply_diff` recreates the containers inserted by a diff, but this
    /// version cannot create a container whose type it doesn't know
    /// ([ContainerType::Unknown], e.g. `cid:0@1:Unknown(9)`). Such values may
    /// only appear where nothing is created: mergeable map children (only the
    /// marker is written), MovableList moves of an element already in the
    /// list, diffs targeting an unknown container (a no-op), and diffs that
    /// `_apply_diff` skips because their target is unreachable.
    ///
    /// With a full-state `plan`, the planned edits are checked instead of the
    /// full states. Nothing is applied here, but the apply loop changes its own
    /// inputs as it goes, so this predicts those changes from the whole batch:
    ///
    /// - fresh: container values of Map/List diffs, the metas of recreated
    ///   tree nodes, and mergeable children of those are recreated under a new
    ///   id (`container_remap`). Their diffs then apply to a new, empty
    ///   container and are never skipped.
    /// - revived: writing a mergeable child's marker makes it and its
    ///   descendants reachable again.
    /// - removed: a diff that deletes a child container makes it and its
    ///   descendants unreachable for the diffs after it.
    ///
    /// Every prediction errs towards checking a diff (a false rejection),
    /// never towards skipping it, so an accepted batch cannot fail on an
    /// unknown container half way through. Returns the batch ids of the
    /// entries holding unknown containers that it predicted to be skipped;
    /// `_apply_diff` asserts that in debug builds.
    fn check_apply_diff_creates_no_unknown_container(
        &self,
        diff: &DiffBatch,
        plan: Option<&FullStatePlan>,
        container_remap: &FxHashMap<ContainerID, ContainerID>,
        skip_unreachable: bool,
        movable_plans: &FxHashMap<ContainerID, crate::undo::movable_list::Plan>,
    ) -> LoroResult<FxHashSet<ContainerID>> {
        let mut predicted_skips = FxHashSet::default();
        if !diff.iter().any(|(_, d)| diff_has_unknown_value(d)) {
            return Ok(predicted_skips);
        }

        // The batch as the loop applies it
        let entries: Vec<(&ContainerID, &crate::event::Diff)> = diff
            .iter()
            .filter_map(|(id, d)| match plan.and_then(|p| p.aligned.get(id)) {
                Some(p) => p.edit.as_ref().map(|edit| (id, edit)),
                None => Some((id, d)),
            })
            .collect();
        let empty = FxHashSet::default();
        let always_applied = plan.map_or(&empty, |p| &p.always_applied);

        // A handler for an existing container, without materializing roots
        // that have no state yet
        let existing = |id: &ContainerID| -> Option<Handler> {
            if id.is_root() && !id.is_mergeable() && self.arena.id_to_idx(id).is_none() {
                return None;
            }
            self.has_container(id)
                .then(|| Handler::new_attached(id.clone(), self.clone()))
        };

        let mut fresh: FxHashSet<ContainerID> = plan.map(|p| p.fresh.clone()).unwrap_or_default();
        let mut revived: FxHashSet<ContainerID> = FxHashSet::default();
        // Container -> index of the first diff in the batch that removes it
        let mut removed: FxHashMap<ContainerID, usize> = FxHashMap::default();
        for (i, &(id, diff)) in entries.iter().enumerate() {
            let (target, _) = resolve_container_remap(id.clone(), container_remap);
            let handler = if fresh.contains(id) || target.is_unknown() {
                None
            } else {
                existing(&target)
            };
            match diff {
                crate::event::Diff::Map(map) => {
                    let map_handler = handler.and_then(|h| h.into_map().ok());
                    for (key, v) in map.updated.iter() {
                        if let Some(LoroValue::Container(old)) =
                            map_handler.as_ref().and_then(|m| m.get(key))
                        {
                            if !old.is_mergeable() {
                                removed.entry(old).or_insert(i);
                            }
                        }
                        if let Some(c) = v.value.as_ref().and_then(diff_value_container_id) {
                            if c.is_mergeable() {
                                // The loop writes the marker of `mergeable(target,
                                // key)` and remaps `c` to it. Unless that is `c`
                                // itself, the child is another container: new (under a
                                // recreated map) or one this check can't see into (a
                                // parent the undo manager already remapped, or a value
                                // under another key). Check it as new and empty.
                                let effective =
                                    ContainerID::new_mergeable(&target, key, c.container_type());
                                if fresh.contains(id)
                                    || effective
                                        != resolve_container_remap(c.clone(), container_remap).0
                                {
                                    fresh.insert(c.clone());
                                }
                                revived.insert(c);
                            } else {
                                fresh.insert(c);
                            }
                        }
                    }
                }
                crate::event::Diff::List(delta) => {
                    if let Some(native) = movable_plans.get(id).and_then(|p| p.preflight()) {
                        fresh.extend(native.creates.iter().cloned());
                        for child in &native.removes {
                            removed.entry(child.clone()).or_insert(i);
                        }
                        continue;
                    }
                    let values: FxHashSet<ContainerID> = inserted_container_ids(delta)
                        .map(|c| resolve_container_remap(c, container_remap).0)
                        .collect();
                    let get = |i: usize| match &handler {
                        Some(Handler::List(l)) => l.get(i),
                        Some(Handler::MovableList(l)) => l.get(i),
                        _ => None,
                    };
                    if handler.is_some() {
                        let movable = matches!(handler, Some(Handler::MovableList(_)));
                        let mut index = 0;
                        for item in delta.iter() {
                            match item {
                                loro_delta::DeltaItem::Retain { len, .. } => index += len,
                                loro_delta::DeltaItem::Replace { delete, .. } => {
                                    for j in index..index + delete {
                                        if let Some(LoroValue::Container(old)) = get(j) {
                                            // A MovableList may move it instead
                                            if !movable || !values.contains(&old) {
                                                removed.entry(old).or_insert(i);
                                            }
                                        }
                                    }
                                    index += delete;
                                }
                            }
                        }
                    }
                    // Moves are counted too, which can only check more
                    fresh.extend(inserted_container_ids(delta));
                }
                crate::event::Diff::Tree(tree) => {
                    for item in tree.diff.iter() {
                        let meta = item.target.associated_meta_container();
                        if matches!(item.action, crate::delta::TreeExternalDiff::Delete { .. }) {
                            if handler.is_some() && !container_remap.contains_key(&meta) {
                                removed.entry(meta).or_insert(i);
                            }
                        } else {
                            fresh.insert(meta);
                        }
                    }
                }
                _ => {}
            }
        }

        for (i, &(id, diff)) in entries.iter().enumerate() {
            if !diff_has_unknown_value(diff) {
                continue;
            }
            let (target, remapped) = resolve_container_remap(id.clone(), container_remap);
            if target.is_unknown() {
                // `Handler::Unknown` ignores diffs
                continue;
            }

            let handler = if fresh.contains(id) {
                None
            } else {
                existing(&target)
            };
            if handler.is_some()
                && skip_unreachable
                && !remapped
                && !always_applied.contains(&target)
            {
                let chain = self.arena_ancestors_and_self(&target);
                let skipped = !chain.iter().any(|c| revived.contains(c))
                    && (!self.state.lock().get_reachable(&target)
                        || chain.iter().any(|c| removed.get(c).is_some_and(|&j| j < i)));
                if skipped {
                    // `_apply_diff` skips this diff
                    predicted_skips.insert(id.clone());
                    continue;
                }
            }

            let created = match diff {
                crate::event::Diff::Map(map) => map
                    .updated
                    .values()
                    .filter_map(|v| v.value.as_ref().and_then(diff_value_container_id))
                    .find(|c| c.is_unknown() && !c.is_mergeable()),
                crate::event::Diff::List(delta)
                    if movable_plans.get(id).and_then(|p| p.preflight()).is_some() =>
                {
                    let _ = delta;
                    movable_plans[id]
                        .preflight()
                        .and_then(|p| p.creates.iter().find(|c| c.is_unknown()).cloned())
                }
                crate::event::Diff::List(delta) => match &handler {
                    // Only an existing MovableList can move an element instead
                    // of creating it
                    Some(Handler::MovableList(list)) => {
                        list.unknown_container_created_by_delta(delta, container_remap)?
                    }
                    _ => inserted_container_ids(delta).find(|c| c.is_unknown()),
                },
                _ => None,
            };
            if let Some(c) = created {
                return Err(crate::handler::unknown_container_creation_err(
                    c.container_type(),
                ));
            }
        }

        Ok(predicted_skips)
    }

    /// `id` and its ancestors by arena parent, whether they are visible or
    /// not (a hidden mergeable child has no path from a root, but its
    /// descendants become reachable again when a batch revives it).
    fn arena_ancestors_and_self(&self, id: &ContainerID) -> Vec<ContainerID> {
        let mut ans = Vec::new();
        let idx = match self.arena.id_to_idx(id) {
            Some(idx) => Some(idx),
            None => {
                let mut state = self.state.lock();
                if state.does_container_exist(id) {
                    state.ensure_container(id);
                }
                drop(state);
                self.arena.id_to_idx(id)
            }
        };
        if let Some(mut idx) = idx {
            while let Some(parent) = self.arena.get_parent(idx) {
                if let Some(parent_id) = self.arena.idx_to_id(parent) {
                    ans.push(parent_id);
                }
                idx = parent;
            }
        }
        ans.push(id.clone());
        ans
    }

    /// This is for debugging purpose. It will travel the whole oplog
    #[inline]
    pub fn diagnose_size(&self) {
        self.oplog().lock().diagnose_size();
    }

    #[inline]
    pub fn oplog_frontiers(&self) -> Frontiers {
        self.oplog().lock().frontiers().clone()
    }

    #[inline]
    pub fn state_frontiers(&self) -> Frontiers {
        self.state.lock().frontiers.clone()
    }

    /// - Ordering::Less means self is less than target or parallel
    /// - Ordering::Equal means versions equal
    /// - Ordering::Greater means self's version is greater than target
    #[inline]
    pub fn cmp_with_frontiers(&self, other: &Frontiers) -> Ordering {
        self.oplog().lock().cmp_with_frontiers(other)
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
        let oplog = self.oplog.lock();
        oplog
            .check_history_parsable()
            .map_err(FrontiersNotIncluded::unreadable)?;
        oplog.cmp_frontiers(a, b)
    }

    pub fn subscribe_root(&self, callback: Subscriber) -> Subscription {
        let mut state = self.state.lock();
        if !state.is_recording() {
            state.start_recording();
        }

        self.observer.subscribe_root(callback)
    }

    pub fn subscribe(&self, container_id: &ContainerID, callback: Subscriber) -> Subscription {
        let mut state = self.state.lock();
        if !state.is_recording() {
            state.start_recording();
        }

        self.observer.subscribe(container_id, callback)
    }

    pub fn subscribe_local_update(&self, callback: LocalUpdateCallback) -> Subscription {
        let (sub, activate) = self.local_update_subs.inner().insert((), callback);
        activate();
        sub
    }

    /// Import a batch of updates or snapshots, in any order.
    ///
    /// This is not all-or-nothing. A blob that fails on its own does not undo the
    /// others: the batch returns the error of the last blob that failed, and keeps what
    /// the other blobs imported, including a dep that lets parked changes apply. Only a
    /// failure of the closing checkout rolls the whole batch back. So an `Err` does not
    /// mean that nothing was applied. [`Self::import_batch_detailed`] returns what
    /// happened to each blob.
    // PERF: opt
    #[tracing::instrument(skip_all)]
    pub fn import_batch(&self, bytes: &[Vec<u8>]) -> LoroResult<ImportStatus> {
        self.import_batch_with_history_mode(bytes, false)
    }

    /// Like [`Self::import_batch`], taking owned [`bytes::Bytes`] buffers and
    /// releasing each buffer as soon as that blob finishes importing.
    #[tracing::instrument(skip_all)]
    pub fn import_batch_bytes(&self, bytes: Vec<bytes::Bytes>) -> LoroResult<ImportStatus> {
        self.import_batch_with_history_mode_bytes(bytes, false)
    }

    /// Like [`Self::import_batch`]. `json_lossy` is the same opt-in as
    /// [`Self::import_with_history_mode`].
    pub fn import_batch_with_history_mode(
        &self,
        bytes: &[Vec<u8>],
        json_lossy: bool,
    ) -> LoroResult<ImportStatus> {
        self.check_import_not_nested()?;
        if bytes.is_empty() {
            return Ok(ImportStatus::default());
        }
        if bytes.len() == 1 {
            return self.import_with_history_mode(&bytes[0], Default::default(), json_lossy);
        }

        let values = imported_values_for_mode(json_lossy);
        // Metadata that does not decode fails the batch before anything is imported.
        let mut scratch = BatchUpdateScratch::default();
        let blobs = bytes
            .iter()
            .enumerate()
            .map(|(index, blob)| {
                self.check_import_blob_limit(blob.len())?;
                let meta = BatchBlobSortKey::decode(&ImportInput::Slice(blob), &mut scratch)?;
                Ok((index, meta, BatchBlobSource::Slice(blob.as_slice())))
            })
            .collect::<LoroResult<Vec<_>>>()?;
        drop(scratch);
        self.finish_batch_status(blobs, values)
    }

    /// Like [`Self::import_batch_with_history_mode`], taking owned [`bytes::Bytes`]
    /// buffers and dropping each buffer as soon as its blob finishes importing.
    pub fn import_batch_with_history_mode_bytes(
        &self,
        bytes: Vec<bytes::Bytes>,
        json_lossy: bool,
    ) -> LoroResult<ImportStatus> {
        self.check_import_not_nested()?;
        if bytes.is_empty() {
            return Ok(ImportStatus::default());
        }
        if bytes.len() == 1 {
            let blob = bytes.into_iter().next().expect("checked len == 1");
            return self.import_with_history_mode_bytes(blob, Default::default(), json_lossy);
        }

        let values = imported_values_for_mode(json_lossy);
        let mut scratch = BatchUpdateScratch::default();
        let blobs = bytes
            .into_iter()
            .enumerate()
            .map(|(index, blob)| {
                self.check_import_blob_limit(blob.len())?;
                let meta =
                    BatchBlobSortKey::decode(&ImportInput::Bytes(blob.clone()), &mut scratch)?;
                Ok((index, meta, BatchBlobSource::Bytes(blob)))
            })
            .collect::<LoroResult<Vec<_>>>()?;
        drop(scratch);
        self.finish_batch_status(blobs, values)
    }

    /// Import a batch whose metadata has already been decoded, loading each blob's
    /// payload on demand and dropping each buffer immediately after its turn.
    pub fn import_batch_with_loaders<'a, F>(
        &self,
        blobs: Vec<(ImportBlobMetadata, F)>,
        json_lossy: bool,
    ) -> LoroResult<ImportStatus>
    where
        F: FnOnce() -> LoroResult<bytes::Bytes> + 'a,
    {
        self.check_import_not_nested()?;
        if blobs.is_empty() {
            return Ok(ImportStatus::default());
        }
        if blobs.len() == 1 {
            let (_, load) = blobs.into_iter().next().expect("checked len == 1");
            let bytes = load()?;
            return self.import_with_history_mode_bytes(bytes, Default::default(), json_lossy);
        }

        let values = imported_values_for_mode(json_lossy);
        let sources = blobs
            .into_iter()
            .enumerate()
            .map(|(index, (meta, load))| {
                (index, meta.into(), BatchBlobSource::Loader(Box::new(load)))
            })
            .collect();
        self.finish_batch_status(sources, values)
    }

    fn finish_batch_status(
        &self,
        blobs: Vec<(usize, BatchBlobSortKey, BatchBlobSource<'_>)>,
        values: ImportedValues,
    ) -> LoroResult<ImportStatus> {
        let run = self.import_blobs_as_batch(blobs, values)?;
        if let Some(err) = run.rollback_error {
            return Err(err);
        }
        // The last blob to fail, in the order they were imported.
        if let Some(err) = run
            .results
            .into_iter()
            .rev()
            .find_map(|(_, result)| result.err())
        {
            return Err(err);
        }

        Ok(ImportStatus {
            success: run.success,
            pending: non_empty(run.pending),
        })
    }

    /// Import a batch as [`Self::import_batch`] does, and return what happened to each
    /// blob instead of the error of one of them.
    ///
    /// - `Err`: the call was refused and no blob was tried. That is
    ///   [`LoroError::ImportWhenInTxn`], while a manual [`Transaction`] is open or when
    ///   called from an event or pre-commit callback of this doc.
    /// - `Ok`: [`ImportBatchDetail::blobs`] has one [`ImportBlobOutcome`] per blob, in
    ///   the order of `bytes`. A blob that fails on its own is `Failed` and the others
    ///   are imported.
    ///
    /// Only the closing checkout applies a batch to the state. When it fails, the whole
    /// batch is rolled back and the doc is as it was before the call. The error is then
    /// in [`ImportBatchDetail::rollback_error`], and every blob that had imported is
    /// `RolledBack`. That checkout checks the batch as a whole, so it cannot say which
    /// blob it rejected: import them one by one to find it. A blob that failed on its
    /// own keeps `Failed`.
    ///
    /// It differs from `import_batch` for one input: a blob whose metadata does not
    /// decode ([`Self::decode_import_blob_meta`]) is `Failed` and the others are imported,
    /// where `import_batch` returns that error before it imports anything. When none of
    /// them decode, this returns before [`Self::import_blobs_as_batch`], as
    /// `import_batch` does: that section would commit the local transaction and, with
    /// detached editing, renew the peer id. A batch of one is a plain import in both,
    /// which the state checks at once, so its failure is `Failed` and never a rollback.
    pub fn import_batch_detailed(&self, bytes: &[Vec<u8>]) -> LoroResult<ImportBatchDetail> {
        self.import_batch_detailed_with_history_mode(bytes, false)
    }

    /// Like [`Self::import_batch_detailed`], taking owned [`bytes::Bytes`] buffers
    /// and dropping each buffer immediately after its blob finishes importing.
    pub fn import_batch_detailed_bytes(
        &self,
        bytes: Vec<bytes::Bytes>,
    ) -> LoroResult<ImportBatchDetail> {
        self.import_batch_detailed_with_history_mode_bytes(bytes, false)
    }

    /// Like [`Self::import_batch_detailed`]. `json_lossy` is the same opt-in as
    /// [`Self::import_with_history_mode`].
    pub fn import_batch_detailed_with_history_mode(
        &self,
        bytes: &[Vec<u8>],
        json_lossy: bool,
    ) -> LoroResult<ImportBatchDetail> {
        self.check_import_not_nested()?;
        let values = imported_values_for_mode(json_lossy);
        if bytes.is_empty() {
            return Ok(self.empty_batch_detail());
        }
        if bytes.len() == 1 {
            return self.import_single_detailed(|| Ok(ImportInput::Slice(&bytes[0])), values);
        }

        let mut scratch = BatchUpdateScratch::default();
        let mut outcomes: Vec<Option<ImportBlobOutcome>> = Vec::with_capacity(bytes.len());
        let mut blobs = Vec::with_capacity(bytes.len());
        for (index, blob) in bytes.iter().enumerate() {
            match self
                .check_import_blob_limit(blob.len())
                .and_then(|()| BatchBlobSortKey::decode(&ImportInput::Slice(blob), &mut scratch))
            {
                Ok(meta) => {
                    blobs.push((index, meta, BatchBlobSource::Slice(blob.as_slice())));
                    outcomes.push(None);
                }
                // Unlike `import_batch`, the other blobs are still imported.
                Err(err) => outcomes.push(Some(ImportBlobOutcome::Failed(err))),
            }
        }
        drop(scratch);
        self.finish_batch_detailed(blobs, outcomes, values)
    }

    /// Like [`Self::import_batch_detailed_with_history_mode`], taking owned
    /// [`bytes::Bytes`] buffers and dropping each buffer right after its turn.
    pub fn import_batch_detailed_with_history_mode_bytes(
        &self,
        bytes: Vec<bytes::Bytes>,
        json_lossy: bool,
    ) -> LoroResult<ImportBatchDetail> {
        self.import_batch_detailed_with_blob_results(bytes.into_iter().map(Ok), json_lossy)
    }

    /// Detailed owned batch whose buffers can fail admission before allocation.
    /// Keeps per-blob errors and the single-blob import boundary intact.
    pub fn import_batch_detailed_with_blob_results<I>(
        &self,
        bytes: I,
        json_lossy: bool,
    ) -> LoroResult<ImportBatchDetail>
    where
        I: IntoIterator<Item = LoroResult<bytes::Bytes>>,
        I::IntoIter: ExactSizeIterator,
    {
        self.check_import_not_nested()?;
        let values = imported_values_for_mode(json_lossy);
        let mut bytes = bytes.into_iter();
        let len = bytes.len();
        if len == 0 {
            return Ok(self.empty_batch_detail());
        }
        if len == 1 {
            let blob = bytes.next().expect("checked len == 1");
            return self.import_single_detailed(|| blob.map(ImportInput::Bytes), values);
        }

        let mut scratch = BatchUpdateScratch::default();
        let mut outcomes: Vec<Option<ImportBlobOutcome>> = Vec::with_capacity(len);
        let mut blobs = Vec::with_capacity(len);
        for (index, result) in bytes.enumerate() {
            let blob = match result {
                Ok(bytes) => bytes,
                Err(err) => {
                    outcomes.push(Some(ImportBlobOutcome::Failed(err)));
                    continue;
                }
            };
            match self.check_import_blob_limit(blob.len()).and_then(|()| {
                BatchBlobSortKey::decode(&ImportInput::Bytes(blob.clone()), &mut scratch)
            }) {
                Ok(meta) => {
                    blobs.push((index, meta, BatchBlobSource::Bytes(blob)));
                    outcomes.push(None);
                }
                Err(err) => outcomes.push(Some(ImportBlobOutcome::Failed(err))),
            }
        }
        drop(scratch);
        self.finish_batch_detailed(blobs, outcomes, values)
    }

    /// Detailed counterpart of [`Self::import_batch_with_loaders`].
    pub fn import_batch_detailed_with_loaders<'a, F>(
        &self,
        blobs: Vec<(LoroResult<ImportBlobMetadata>, F)>,
        json_lossy: bool,
    ) -> LoroResult<ImportBatchDetail>
    where
        F: FnOnce() -> LoroResult<bytes::Bytes> + 'a,
    {
        self.check_import_not_nested()?;
        let values = imported_values_for_mode(json_lossy);
        if blobs.is_empty() {
            return Ok(self.empty_batch_detail());
        }
        if blobs.len() == 1 {
            let (_, load) = blobs.into_iter().next().expect("checked len == 1");
            return self.import_single_detailed(|| load().map(ImportInput::Bytes), values);
        }

        let mut outcomes: Vec<Option<ImportBlobOutcome>> = Vec::with_capacity(blobs.len());
        let mut valid = Vec::with_capacity(blobs.len());
        for (index, (meta, load)) in blobs.into_iter().enumerate() {
            match meta {
                Ok(meta) => {
                    valid.push((index, meta.into(), BatchBlobSource::Loader(Box::new(load))));
                    outcomes.push(None);
                }
                Err(err) => outcomes.push(Some(ImportBlobOutcome::Failed(err))),
            }
        }
        self.finish_batch_detailed(valid, outcomes, values)
    }

    fn empty_batch_detail(&self) -> ImportBatchDetail {
        let pending = self.oplog.lock().pending_changes.version_range();
        ImportBatchDetail {
            blobs: Vec::new(),
            status: ImportStatus {
                success: Default::default(),
                pending: non_empty(pending),
            },
            rollback_error: None,
        }
    }

    fn import_single_detailed<'a>(
        &self,
        load_input: impl FnOnce() -> LoroResult<ImportInput<'a>>,
        values: ImportedValues,
    ) -> LoroResult<ImportBatchDetail> {
        // A plain import, as in `import_batch`. The pending range is read under
        // the same transaction lock.
        let (result, pending) = self.with_barrier(|| {
            let result = load_input()
                .and_then(|input| self._import_with_input(input, Default::default(), values));
            (result, self.oplog.lock().pending_changes.version_range())
        });
        let (blob, success) = match result {
            Ok(status) => {
                let success = status.success.clone();
                (ImportBlobOutcome::Imported(status), success)
            }
            // Only the transaction checks return this, before the blob is tried.
            Err(LoroError::ImportWhenInTxn) => return Err(LoroError::ImportWhenInTxn),
            Err(err) => (ImportBlobOutcome::Failed(err), Default::default()),
        };
        Ok(ImportBatchDetail {
            blobs: vec![blob],
            status: ImportStatus {
                success,
                pending: non_empty(pending),
            },
            rollback_error: None,
        })
    }

    fn finish_batch_detailed(
        &self,
        blobs: Vec<(usize, BatchBlobSortKey, BatchBlobSource<'_>)>,
        mut outcomes: Vec<Option<ImportBlobOutcome>>,
        values: ImportedValues,
    ) -> LoroResult<ImportBatchDetail> {
        // Nothing decoded, so there is nothing to import. `import_batch` returns
        // before this critical section; entering it would commit the local
        // transaction and, when detached editing is on, renew the peer id.
        if blobs.is_empty() {
            let pending = self.oplog.lock().pending_changes.version_range();
            return Ok(ImportBatchDetail {
                blobs: outcomes
                    .into_iter()
                    .map(|outcome| outcome.expect("every blob failed to decode"))
                    .collect(),
                status: ImportStatus {
                    success: Default::default(),
                    pending: non_empty(pending),
                },
                rollback_error: None,
            });
        }

        let run = self.import_blobs_as_batch(blobs, values)?;
        let rolled_back = run.rollback_error.is_some();
        for (index, result) in run.results {
            outcomes[index] = Some(match result {
                Err(err) => ImportBlobOutcome::Failed(err),
                // Nothing of the batch is left, so this blob is not in the doc either.
                Ok(_) if rolled_back => ImportBlobOutcome::RolledBack,
                Ok(status) => ImportBlobOutcome::Imported(status),
            });
        }

        Ok(ImportBatchDetail {
            blobs: outcomes
                .into_iter()
                .map(|outcome| outcome.expect("the batch imports every blob it is given"))
                .collect(),
            status: ImportStatus {
                success: run.success,
                pending: non_empty(run.pending),
            },
            rollback_error: run.rollback_error,
        })
    }

    /// The critical section of a batch import, for blobs whose metadata decoded:
    /// `(index in the caller's input, metadata, payload source)`.
    ///
    /// Returns `Err` only when the batch is refused before anything changes. A blob
    /// that fails, and a failed closing checkout, are in the [`BatchRun`].
    fn import_blobs_as_batch(
        &self,
        mut blobs: Vec<(usize, BatchBlobSortKey, BatchBlobSource<'_>)>,
        values: ImportedValues,
    ) -> LoroResult<BatchRun> {
        let mut success = VersionRange::default();
        blobs.sort_by(|a, b| {
            a.1.mode
                .cmp(&b.1.mode)
                .then(b.1.change_num.cmp(&a.1.change_num))
        });

        let (options, txn) = self.implicit_commit_then_stop();
        // Before anything changes. Holding the transaction lock keeps a manual
        // transaction from starting until the batch ends, so this is checked once.
        if self.state.lock().is_in_txn() {
            drop(txn);
            self.renew_txn_if_auto_commit(options);
            return Err(LoroError::ImportWhenInTxn);
        }

        // Why we should keep locking `txn` here
        //
        // In a multi-threaded environment, `import_batch` used to drop the txn lock
        // (via `commit_then_stop` + `drop(txn)`) and call `detach()`/`checkout_to_latest()`
        // around the batch import. That created a race where another thread could
        // start or renew the auto-commit txn and perform local edits while we were
        // importing and temporarily detached. Those interleaved local edits could
        // violate invariants between `OpLog` and `DocState` (e.g., state being
        // updated when we expect it not to, missed events, or inconsistent
        // frontiers), as exposed by the loom test `local_edits_during_batch_import`.
        //
        // The fix is to hold the txn mutex for the entire critical section:
        // - Stop the current txn and keep the mutex guard.
        // - Force-detach with `set_detached(true)` (avoids `detach()` side effects),
        //   then run each `_import_with(...)` while detached so imports only touch
        //   the `OpLog`.
        // - After importing, reattach by checking out to latest and renew the txn in
        //   `BatchImportGuard::finish`, which keeps the mutex held while (re)starting
        //   the auto-commit txn.
        //
        // Holding the lock ensures no concurrent thread can create/renew a txn and
        // do local edits in the middle of the batch import, making the whole
        // operation atomic with respect to local edits.
        //
        // The force-detach must be undone on *every* exit path. A blob that returns
        // `Err`, and just as importantly a panic out of decode/apply, used to skip the
        // reattach below and strand the document in detached mode, where it silently
        // stops reflecting later imports and local edits. `BatchImportGuard::finish`
        // owns that cleanup and the blob loop runs inside `catch_unwind` so it also
        // runs before a panic is re-raised.
        self.touch_access_tick();
        let was_attached = !self.is_detached();
        let linear_before = wasm_linear_memory_bytes();
        let has_doc_budget = self.has_max_doc_bytes();
        // A snapshot can initialize state lazily, just as in `import`, even
        // with parked changes. Keep their original claims for batch rollback.
        // Keep the pre-snapshot arena checkpoint so a failed closing checkout
        // can discard the snapshot's state as well as the following updates.
        let snapshot_checkpoint = if was_attached
            && blobs.first().is_some_and(|blob| {
                matches!(
                    blob.1.mode,
                    encoding::EncodedBlobMode::Snapshot
                        | encoding::EncodedBlobMode::ShallowSnapshot
                )
            }) {
            let oplog = self.oplog.lock();
            let mut state = self.state.lock();
            (oplog.dag().is_empty() && state.can_import_snapshot()).then(|| BatchSnapshotRollback {
                arena: oplog.arena.checkpoint_for_rollback(),
                empty_containers: state.store.iter_all_container_ids().collect(),
                pending: oplog.pending_changes.clone(),
            })
        } else {
            None
        };
        let (pre_batch_frontiers, has_created_journal) =
            if was_attached && snapshot_checkpoint.is_none() && has_doc_budget {
                let mut state = self.state.lock();
                state.store.begin_created_journal();
                (state.frontiers.clone(), true)
            } else {
                (Frontiers::default(), false)
            };
        self.set_detached(true);
        let has_import_rollback = {
            let mut oplog = self.oplog.lock();
            oplog.batch_importing = true;
            // The history-only path touches just the `OpLog`, so a rollback
            // scope spanning the whole batch lets `BatchImportGuard::finish` restore
            // the pre-batch version if the closing reattach cannot apply the imported
            // changes to `DocState` or `MemoryBudget::max_doc_bytes` is exceeded.
            let need_scope = (was_attached && snapshot_checkpoint.is_none())
                || (!was_attached && (has_doc_budget || oplog.has_max_pending_bytes()));
            if need_scope {
                oplog.begin_import_rollback();
                true
            } else {
                false
            }
        };

        let mut guard = BatchImportGuard {
            doc: self,
            was_attached,
            has_doc_budget,
            has_import_rollback,
            has_created_journal,
            pre_batch_frontiers,
            txn: Some(txn),
            options,
            snapshot_checkpoint,
            linear_before,
            #[cfg(debug_assertions)]
            finished: false,
        };

        // Catch here rather than in a `Drop`: releasing a mutex guard while a panic is
        // in flight poisons that mutex, so the reattach has to happen after unwinding
        // has stopped. Under `panic = "abort"` (e.g. wasm) a genuine panic still
        // terminates execution; only returned errors can be recovered there.
        let loop_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut results = Vec::with_capacity(blobs.len());
            for (order, (index, _meta, source)) in blobs.into_iter().enumerate() {
                #[cfg(test)]
                take_panic_at_batch_import_blob_for_test();
                let loaded = match source {
                    BatchBlobSource::Slice(data) => Ok(ImportInput::Slice(data)),
                    BatchBlobSource::Bytes(data) => Ok(ImportInput::Bytes(data)),
                    BatchBlobSource::Loader(load) => load().map(ImportInput::Bytes),
                };
                let result = if let Some(checkpoint) =
                    guard.snapshot_checkpoint.as_ref().filter(|_| order == 0)
                {
                    // Do not emit the snapshot event until the closing checkout
                    // succeeds. Its state is part of the batch's rollback scope.
                    let result = loaded.and_then(|input| match input {
                        ImportInput::Slice(data) => {
                            guard.doc.check_import_blob_limit(data.len())?;
                            parse_header_and_body(data, true).and_then(|parsed| {
                                guard.doc.init_snapshot_preserving_pending(
                                    parsed.mode,
                                    parsed.body,
                                    Default::default(),
                                    values,
                                )
                            })
                        }
                        ImportInput::Bytes(data) => {
                            guard.doc.check_import_blob_limit(data.len())?;
                            parse_header_and_body_bytes(data, true).and_then(|parsed| {
                                guard.doc.init_snapshot_preserving_pending_bytes(
                                    parsed.mode,
                                    parsed.body,
                                    Default::default(),
                                    values,
                                )
                            })
                        }
                    });
                    if result.is_err() {
                        // The standalone decoder resets a failed snapshot to an
                        // empty store. Restore pre-existing empty handles, too.
                        let mut state = guard.doc.state.lock();
                        for id in &checkpoint.empty_containers {
                            state.ensure_container(id);
                        }
                    }
                    let mut oplog = guard.doc.oplog.lock();
                    // Snapshot decoding installs a new DAG; start the ordinary
                    // suffix journal only after that installation (or its reset).
                    oplog.batch_importing = true;
                    oplog.begin_import_rollback();
                    result
                } else {
                    loaded.and_then(|input| {
                        guard
                            .doc
                            .import_without_emitting_input(input, Default::default(), values)
                    })
                };
                if let Ok(s) = &result {
                    for (peer, (start, end)) in s.success.iter() {
                        match success.0.entry(*peer) {
                            Entry::Occupied(mut e) => {
                                e.get_mut().1 = *end.max(&e.get().1);
                            }
                            Entry::Vacant(e) => {
                                e.insert((*start, *end));
                            }
                        }
                    }
                }
                results.push((index, result));
            }
            results
        }));

        let results = match loop_result {
            Ok(results) => results,
            Err(payload) => {
                // Reattach first, then re-raise the original panic. `finish` can panic
                // itself when the interrupted blob poisoned a doc lock; that document
                // is unusable either way, so keep the original payload.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = guard.finish();
                }));
                std::panic::resume_unwind(payload);
            }
        };

        let (rollback_error, pending) = guard.finish();
        if rollback_error.is_some() {
            success = VersionRange::default();
        }
        Ok(BatchRun {
            results,
            success,
            pending,
            rollback_error,
        })
    }

    /// Get shallow value of the document.
    #[inline]
    pub fn get_value(&self) -> LoroValue {
        self.touch_access_tick();
        self.state.lock().get_value()
    }

    /// Get deep value of the document.
    #[inline]
    pub fn get_deep_value(&self) -> LoroValue {
        self.touch_access_tick();
        self.state.lock().get_deep_value()
    }

    /// Get deep value of the document with container id
    #[inline]
    pub fn get_deep_value_with_id(&self) -> LoroValue {
        self.touch_access_tick();
        self.state.lock().get_deep_value_with_id()
    }

    /// [`Self::try_checkout_to_latest`] for a caller that cannot handle an error.
    ///
    /// # Panics
    ///
    /// Where that method returns an error, once no lock is held, so the doc stays usable.
    pub fn checkout_to_latest(&self) {
        self.try_checkout_to_latest()
            .unwrap_or_else(|err| panic!("checkout to the latest version failed: {err}"));
    }

    /// Check the state out to the latest version of the op log and attach it.
    ///
    /// A detached import does not touch the state, so history that the state rejects,
    /// such as a list insert past the end from a forged update, can be in the op log
    /// and fail here. The doc then stays detached at its version and usable. It also
    /// fails while a manual transaction is open, see [`Self::checkout`].
    pub fn try_checkout_to_latest(&self) -> LoroResult<()> {
        let (options, guard) = self.implicit_commit_then_stop();
        if !self.is_detached() {
            drop(guard);
            self.renew_txn_if_auto_commit(options);
            return Ok(());
        }

        let result = self._checkout_to_latest_without_commit(true);
        if result.is_ok() {
            self.emit_events();
        }
        drop(guard);
        self.renew_txn_if_auto_commit(options);
        result
    }

    /// NOTE: The caller of this method should ensure the txn is locked and set to None
    pub(crate) fn _checkout_to_latest_without_commit(
        &self,
        to_commit_then_renew: bool,
    ) -> LoroResult<()> {
        self._checkout_to_latest_without_commit_with_event(
            to_commit_then_renew,
            "checkout".into(),
            EventTriggerKind::Checkout,
            true,
        )
    }

    pub(crate) fn _checkout_to_latest_without_commit_as_import(
        &self,
        to_commit_then_renew: bool,
        origin: InternalString,
    ) -> LoroResult<()> {
        self._checkout_to_latest_without_commit_with_event(
            to_commit_then_renew,
            origin,
            EventTriggerKind::Import,
            false,
        )
    }

    fn _checkout_to_latest_without_commit_with_event(
        &self,
        to_commit_then_renew: bool,
        origin: InternalString,
        triggered_by: EventTriggerKind,
        emit_events: bool,
    ) -> LoroResult<()> {
        tracing::info_span!("CheckoutToLatest", peer = self.peer_id()).in_scope(|| {
            let f = self.oplog_frontiers();
            let this = &self;
            let frontiers = &f;
            this._checkout_without_emitting_with_event(
                frontiers,
                false,
                to_commit_then_renew,
                origin,
                triggered_by,
            )?;
            // We don't need to shrink frontiers because oplog's frontiers are already shrinked.
            // Snapshot replay is part of a larger atomic import. Its events
            // must stay queued until pending replay / batch validation succeeds.
            if emit_events {
                this.emit_events();
            }
            if this.config.detached_editing() {
                this.renew_peer_id();
            }

            self.set_detached(false);
            Ok(())
        })
    }

    /// Checkout [DocState] to a specific version.
    ///
    /// This will make the current [DocState] detached from the latest version of [OpLog].
    /// Any further import will not be reflected on the [DocState], until user call [LoroDoc::attach()]
    ///
    /// Returns [`LoroError::TransactionError`] while a manual [`Transaction`] is open:
    /// its ops are in the DAG and not in the change store yet, so the history cannot
    /// be read. Commit it first.
    pub fn checkout(&self, frontiers: &Frontiers) -> LoroResult<()> {
        self.touch_access_tick();
        // Checked here and not in `_checkout_without_emitting`: undo, `checkout_to_latest`
        // and the exporters call that and cannot return this error.
        self.oplog.lock().check_history_parsable()?;
        let was_detached = self.is_detached();
        let (options, guard) = self.implicit_commit_then_stop();
        let result = self._checkout_without_emitting(frontiers, true, true);
        if result.is_ok() {
            self.emit_events();
        }
        drop(guard);
        if self.config.detached_editing() {
            if result.is_ok() {
                self.renew_peer_id();
            }
            self.renew_txn_if_auto_commit(options);
        } else if result.is_err() {
            if !was_detached {
                self.renew_txn_if_auto_commit(options);
            }
        } else if !self.is_detached() {
            self.renew_txn_if_auto_commit(options);
        }

        result
    }

    /// NOTE: The caller of this method should ensure the txn is locked and set to None
    #[instrument(level = "info", skip(self))]
    pub(crate) fn _checkout_without_emitting(
        &self,
        frontiers: &Frontiers,
        to_shrink_frontiers: bool,
        to_commit_then_renew: bool,
    ) -> Result<(), LoroError> {
        self._checkout_without_emitting_with_event(
            frontiers,
            to_shrink_frontiers,
            to_commit_then_renew,
            "checkout".into(),
            EventTriggerKind::Checkout,
        )
    }

    fn _checkout_without_emitting_with_event(
        &self,
        frontiers: &Frontiers,
        to_shrink_frontiers: bool,
        _to_commit_then_renew: bool,
        origin: InternalString,
        triggered_by: EventTriggerKind,
    ) -> Result<(), LoroError> {
        if !self.txn.is_locked() {
            return Err(LoroError::TransactionError(
                "checkout requires the transaction mutex to be held"
                    .to_string()
                    .into_boxed_str(),
            ));
        }
        // Barriers end the auto-commit transaction, so this is a manual one.
        if self.state.lock().is_in_txn() {
            return Err(LoroError::TransactionError(
                "cannot check out while a transaction is open; commit it first"
                    .to_string()
                    .into_boxed_str(),
            ));
        }
        let from_frontiers = self.state_frontiers();
        loro_common::info!(
            "checkout from={:?} to={:?} cur_vv={:?}",
            from_frontiers,
            frontiers,
            self.oplog_vv()
        );

        if &from_frontiers == frontiers {
            // Already there: keep the attached/detached mode. A detached doc at
            // the latest version stays detached, as it does when it gets there
            // from another version; `checkout_to_latest` attaches (loro-dev/loro#1048).
            return Ok(());
        }

        let oplog = self.oplog.lock();
        if oplog.dag.is_before_shallow_root(frontiers) {
            return Err(LoroError::SwitchToVersionBeforeShallowRoot);
        }

        let frontiers = if to_shrink_frontiers {
            crate::version::try_shrink_frontiers(frontiers, &oplog.dag)?
        } else {
            frontiers.clone()
        };

        if from_frontiers == frontiers {
            return Ok(());
        }

        let mut state = self.state.lock();
        let mut calc = self.diff_calculator.lock();
        for i in frontiers.iter() {
            if !oplog.dag.contains(i) {
                return Err(LoroError::FrontiersNotFound(i));
            }
        }

        let before = oplog
            .dag
            .try_frontiers_to_vv(&state.frontiers)?
            .ok_or_else(|| {
                LoroError::NotFoundError(
                    format!(
                        "Cannot find the current state version {:?}",
                        state.frontiers
                    )
                    .into_boxed_str(),
                )
            })?;
        let Some(after) = &oplog.dag.try_frontiers_to_vv(&frontiers)? else {
            return Err(LoroError::NotFoundError(
                format!("Cannot find the specified version {:?}", frontiers).into_boxed_str(),
            ));
        };

        let (diff, diff_mode) = calc.try_calc_diff_internal(
            &oplog,
            &before,
            &state.frontiers,
            after,
            &frontiers,
            None,
        )?;
        self.set_detached(true);
        // A forward checkout can revive map/list children queried while state
        // lagged behind history, even when the public event is an import.
        state.clear_dead_container_cache();
        state.apply_diff(
            InternalDocDiff {
                origin,
                diff: Cow::Owned(diff),
                by: triggered_by,
                new_version: Cow::Owned(frontiers.clone()),
                local_inverse: None,
            },
            diff_mode,
        )?;
        // Only at the latest version: every root the calculator has seen is then in
        // the history of the target version. Checking out an older version keeps
        // whatever roots the state already had.
        if after == oplog.vv() {
            materialize_touched_roots(&mut state, &calc, &oplog.arena);
            drop(calc);
        }

        Ok(())
    }

    #[inline]
    pub fn vv_to_frontiers(&self, vv: &VersionVector) -> Frontiers {
        self.oplog.lock().dag.vv_to_frontiers(vv)
    }

    #[inline]
    pub fn frontiers_to_vv(&self, frontiers: &Frontiers) -> Option<VersionVector> {
        self.oplog.lock().dag.frontiers_to_vv(frontiers)
    }

    /// Converts frontiers to a version vector, reporting unreadable history.
    pub fn try_frontiers_to_vv(&self, frontiers: &Frontiers) -> LoroResult<Option<VersionVector>> {
        let oplog = self.oplog.lock();
        oplog.check_history_parsable()?;
        oplog.dag.try_frontiers_to_vv(frontiers)
    }

    /// Converts a version vector to frontiers, reporting unreadable history.
    pub fn try_vv_to_frontiers(&self, vv: &VersionVector) -> LoroResult<Frontiers> {
        let oplog = self.oplog.lock();
        oplog.check_history_parsable()?;
        oplog.dag.try_vv_to_frontiers(vv)
    }

    /// Removes redundant frontiers, reporting unreadable history or an absent id.
    pub fn try_minimize_frontiers(&self, frontiers: &Frontiers) -> LoroResult<Frontiers> {
        let oplog = self.oplog.lock();
        oplog.check_history_parsable()?;
        crate::version::try_shrink_frontiers(frontiers, &oplog.dag)
    }

    /// Reads change metadata, distinguishing unreadable history from an absent id.
    pub fn try_get_change(&self, id: ID) -> LoroResult<Option<ChangeMeta>> {
        let oplog = self.oplog.lock();
        let change = oplog.try_get_change_at(id)?;
        Ok(change.as_deref().map(ChangeMeta::from_change))
    }

    /// Import ops from other doc.
    ///
    /// After `a.merge(b)` and `b.merge(a)`, `a` and `b` will have the same content if they are in attached mode.
    pub fn merge(&self, other: &Self) -> LoroResult<ImportStatus> {
        let updates = other
            .export(ExportMode::updates(&self.oplog_vv()))
            .map_err(LoroError::from)?;
        self.import(&updates)
    }

    pub(crate) fn arena(&self) -> &SharedArena {
        &self.arena
    }

    #[inline]
    pub fn len_ops(&self) -> usize {
        if self.oplog.can_lock_in_this_thread() {
            return self.oplog.lock().visible_op_count_exact();
        }

        self.visible_op_count.load(Acquire)
    }

    #[inline]
    pub fn len_changes(&self) -> usize {
        let oplog = self.oplog.lock();
        oplog.len_changes()
    }

    pub fn config(&self) -> &Configure {
        &self.config
    }

    /// This method compare the consistency between the current doc state
    /// and the state calculated by diff calculator from beginning.
    ///
    /// Panic when it's not consistent
    pub fn check_state_diff_calc_consistency_slow(&self) {
        // #[cfg(any(test, debug_assertions, feature = "test_utils"))]
        {
            static IS_CHECKING: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if IS_CHECKING.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }

            IS_CHECKING.store(true, std::sync::atomic::Ordering::Release);
            let peer_id = self.peer_id();
            let s = info_span!("CheckStateDiffCalcConsistencySlow", ?peer_id);
            let _g = s.enter();
            let options = self.implicit_commit_then_stop().0;
            self.oplog.lock().check_dag_correctness();
            if self.is_shallow() {
                // For shallow documents, we cannot replay from the beginning as the history is not complete.
                //
                // Instead, we:
                // 1. Export the initial state from the GC snapshot.
                // 2. Create a new document and import the initial snapshot.
                // 3. Export updates from the shallow start version vector to the current version.
                // 4. Import these updates into the new document.
                // 5. Compare the states of the new document and the current document.

                // Step 1: Export the initial state from the GC snapshot.
                let initial_snapshot = self
                    .export(ExportMode::state_only(Some(
                        &self.shallow_since_frontiers(),
                    )))
                    .unwrap();

                // Step 2: Create a new document and import the initial snapshot.
                let doc = LoroDoc::new();
                doc.import(&initial_snapshot).unwrap();
                self.checkout(&self.shallow_since_frontiers()).unwrap();
                assert_eq!(self.get_deep_value(), doc.get_deep_value());

                // Step 3: Export updates since the shallow start version vector to the current version.
                let updates = self.export(ExportMode::all_updates()).unwrap();

                // Step 4: Import these updates into the new document.
                doc.import(&updates).unwrap();
                self.checkout_to_latest();

                // Step 5: Checkout to the current state's frontiers and compare the states.
                // doc.checkout(&self.state_frontiers()).unwrap();
                assert_eq!(doc.get_deep_value(), self.get_deep_value());
                let mut calculated_state = doc.app_state().lock();
                let mut current_state = self.app_state().lock();
                current_state.check_is_the_same(&mut calculated_state);
            } else {
                let f = self.state_frontiers();
                let vv = self.oplog().lock().dag.frontiers_to_vv(&f).unwrap();
                let bytes = self.export(ExportMode::updates_till(&vv)).unwrap();
                let doc = Self::new();
                doc.import(&bytes).unwrap();
                let mut calculated_state = doc.app_state().lock();
                let mut current_state = self.app_state().lock();
                current_state.check_is_the_same(&mut calculated_state);
            }

            self.renew_txn_if_auto_commit(options);
            IS_CHECKING.store(false, std::sync::atomic::Ordering::Release);
        }
    }

    pub fn query_pos(&self, pos: &Cursor) -> Result<PosQueryResult, CannotFindRelativePosition> {
        match self.query_pos_internal(pos, true) {
            Ok(ans) => Ok(ans),
            Err(QueryPosError::Missing(err)) => Err(err),
            // `CannotFindRelativePosition` is `Copy` and has no room for the
            // decode error. `HistoryUnreadable` is the non-panicking mapping.
            Err(QueryPosError::History(_)) => Err(CannotFindRelativePosition::HistoryUnreadable),
        }
    }

    /// [`query_pos`](Self::query_pos), preserving a decode error from unreadable history.
    pub fn try_get_cursor_pos(&self, pos: &Cursor) -> LoroResult<PosQueryResult> {
        match self.query_pos_internal(pos, true) {
            Ok(ans) => Ok(ans),
            Err(QueryPosError::History(err)) => Err(err),
            Err(QueryPosError::Missing(err)) => {
                Err(LoroError::NotFoundError(err.to_string().into_boxed_str()))
            }
        }
    }

    /// Get position in a seq container
    pub(crate) fn query_pos_internal(
        &self,
        pos: &Cursor,
        ret_event_index: bool,
    ) -> Result<PosQueryResult, QueryPosError> {
        // Public construction and Serde can bypass Cursor::decode. Reject invalid
        // ids before taking document locks or tracing their history in the DAG.
        if pos.id.is_some_and(|id| id.counter < 0) {
            return Err(CannotFindRelativePosition::IdNotFound.into());
        }

        if !self.has_container(&pos.container) {
            return Err(CannotFindRelativePosition::IdNotFound.into());
        }

        // Cursors can only point into sequence containers
        if !matches!(
            pos.container.container_type(),
            ContainerType::Text | ContainerType::List | ContainerType::MovableList
        ) {
            return Err(CannotFindRelativePosition::IdNotFound.into());
        }

        let mut state = self.state.lock();
        if let Some(ans) = state.get_relative_position(pos, ret_event_index) {
            Ok(PosQueryResult {
                update: None,
                current: AbsolutePosition {
                    pos: ans,
                    side: pos.side,
                },
            })
        } else {
            // We need to trace back to the version where the relative position is valid.
            // The optimal way to find that version is to have succ info like Automerge.
            //
            // But we don't have that info now, so an alternative way is to trace back
            // to version with frontiers of `[pos.id]`. But this may be very slow even if
            // the target is just deleted a few versions ago.
            //
            // What we need is to trace back to the latest version that deletes the target
            // id.

            // commit the txn to make sure we can query the history correctly, preserving options
            drop(state);
            let result = self.with_barrier(|| {
                let oplog = self.oplog().lock();
                // TODO: assert pos.id is not unknown
                if let Some(id) = pos.id {
                    // Ensure the container is registered if it exists lazily
                    if oplog.arena.id_to_idx(&pos.container).is_none() {
                        let mut s = self.state.lock();
                        if !s.does_container_exist(&pos.container) {
                            return Err(CannotFindRelativePosition::ContainerDeleted.into());
                        }
                        s.ensure_container(&pos.container);
                        drop(s);
                    }
                    let idx = oplog.arena.id_to_idx(&pos.container).unwrap();
                    // We know where the target id is when we trace back to the delete_op_id.
                    let Some(delete_op_id) = find_last_delete_op(&oplog, id, idx)? else {
                        if oplog.shallow_since_vv().includes_id(id) {
                            return Err(CannotFindRelativePosition::HistoryCleared.into());
                        }

                        tracing::error!("Cannot find id {}", id);
                        return Err(CannotFindRelativePosition::IdNotFound.into());
                    };
                    // Should use persist mode so that it will force all the diff calculators to use the `checkout` mode
                    let mut diff_calc = DiffCalculator::new(true);
                    let before_frontiers: Frontiers = oplog.dag.find_deps_of_id(delete_op_id);
                    let before = frontiers_to_vv_or_not_found(&oplog, &before_frontiers)?;
                    // TODO: PERF: it doesn't need to calc the effects here
                    diff_calc.try_calc_diff_internal(
                        &oplog,
                        &before,
                        &before_frontiers,
                        oplog.vv(),
                        oplog.frontiers(),
                        Some(&|target| idx == target),
                    )?;
                    // TODO: remove depth info
                    let depth = self.arena.get_depth(idx);
                    let (_, diff_calc) = &mut diff_calc.get_or_create_calc(idx, depth);
                    match diff_calc {
                        crate::diff_calc::ContainerDiffCalculator::Richtext(text) => {
                            let c = text.get_id_latest_pos(id).unwrap();
                            let new_pos = c.pos;
                            let handler = self.get_text(&pos.container);
                            let current_pos = handler.convert_entity_index_to_event_index(new_pos);
                            Ok(PosQueryResult {
                                update: handler.get_cursor(current_pos, c.side),
                                current: AbsolutePosition {
                                    pos: current_pos,
                                    side: c.side,
                                },
                            })
                        }
                        crate::diff_calc::ContainerDiffCalculator::List(list) => {
                            let c = list.get_id_latest_pos(id).unwrap();
                            let new_pos = c.pos;
                            let handler = self.get_list(&pos.container);
                            Ok(PosQueryResult {
                                update: handler.get_cursor(new_pos, c.side),
                                current: AbsolutePosition {
                                    pos: new_pos,
                                    side: c.side,
                                },
                            })
                        }
                        crate::diff_calc::ContainerDiffCalculator::MovableList(list) => {
                            let c = list.get_id_latest_pos(id).unwrap();
                            let new_pos = c.pos;
                            let handler = self.get_movable_list(&pos.container);
                            let new_pos = handler.op_pos_to_user_pos(new_pos);
                            Ok(PosQueryResult {
                                update: handler.get_cursor(new_pos, c.side),
                                current: AbsolutePosition {
                                    pos: new_pos,
                                    side: c.side,
                                },
                            })
                        }
                        crate::diff_calc::ContainerDiffCalculator::Tree(_) => unreachable!(),
                        crate::diff_calc::ContainerDiffCalculator::Map(_) => unreachable!(),
                        #[cfg(feature = "counter")]
                        crate::diff_calc::ContainerDiffCalculator::Counter(_) => unreachable!(),
                        crate::diff_calc::ContainerDiffCalculator::Unknown(_) => unreachable!(),
                    }
                } else {
                    match pos.container.container_type() {
                        ContainerType::Text => {
                            let text = self.get_text(&pos.container);
                            Ok(PosQueryResult {
                                update: Some(Cursor {
                                    id: None,
                                    container: text.id(),
                                    side: pos.side,
                                    origin_pos: text.len_unicode(),
                                }),
                                current: AbsolutePosition {
                                    pos: text.len_event(),
                                    side: pos.side,
                                },
                            })
                        }
                        ContainerType::List => {
                            let list = self.get_list(&pos.container);
                            Ok(PosQueryResult {
                                update: Some(Cursor {
                                    id: None,
                                    container: list.id(),
                                    side: pos.side,
                                    origin_pos: list.len(),
                                }),
                                current: AbsolutePosition {
                                    pos: list.len(),
                                    side: pos.side,
                                },
                            })
                        }
                        ContainerType::MovableList => {
                            let list = self.get_movable_list(&pos.container);
                            Ok(PosQueryResult {
                                update: Some(Cursor {
                                    id: None,
                                    container: list.id(),
                                    side: pos.side,
                                    origin_pos: list.len(),
                                }),
                                current: AbsolutePosition {
                                    pos: list.len(),
                                    side: pos.side,
                                },
                            })
                        }
                        ContainerType::Map | ContainerType::Tree | ContainerType::Unknown(_) => {
                            unreachable!()
                        }
                        #[cfg(feature = "counter")]
                        ContainerType::Counter => unreachable!(),
                    }
                }
            });
            result
        }
    }

    /// Free the history cache that is used for making checkout faster.
    ///
    /// If you use checkout that switching to an old/concurrent version, the history cache will be built.
    /// You can free it by calling this method.
    pub fn free_history_cache(&self) {
        self.oplog.lock().free_history_cache();
    }

    /// Free the cached diff calculator that is used for checkout.
    pub fn free_diff_calculator(&self) {
        *self.diff_calculator.lock() = DiffCalculator::new(true);
    }

    /// If you use checkout that switching to an old/concurrent version, the history cache will be built.
    /// You can free it by calling `free_history_cache`.
    pub fn has_history_cache(&self) -> bool {
        self.oplog.lock().has_history_cache()
    }

    /// Encoded all ops and history cache to bytes and store them in the kv store.
    ///
    /// The parsed ops will be dropped
    #[inline]
    pub fn compact_change_store(&self) {
        self.with_barrier(|| {
            self.oplog.lock().compact_change_store();
        });
    }

    /// Analyze the container info of the doc
    ///
    /// This is used for development and debugging
    #[inline]
    pub fn analyze(&self) -> DocAnalysis {
        DocAnalysis::analyze(self)
    }

    /// Get the path from the root to the container
    pub fn get_path_to_container(&self, id: &ContainerID) -> Option<Vec<(ContainerID, Index)>> {
        let mut state = self.state.lock();
        if state.arena.id_to_idx(id).is_none() {
            if id.is_mergeable() {
                // Mergeable children can be logically active via the parent map marker
                // before they have their own encoded state. Register only the arena edge; do not
                // create container state or change `has_container` semantics.
                state.arena.register_container(id);
            } else if !state.does_container_exist(id) {
                return None;
            } else {
                state.ensure_container(id);
            }
        }
        // The registration above can already be gone: a failed import on another thread
        // rolls back every registration made since it began (loro-dev/loro#1164).
        let idx = state.arena.id_to_idx(id)?;
        state.get_path(idx)
    }

    #[instrument(skip(self))]
    pub fn export(&self, mode: ExportMode) -> Result<Vec<u8>, LoroEncodeError> {
        self.export_inner(mode, true)
    }

    /// With `reject_partial_history`, the export fails if a change block that cannot be
    /// parsed was found, before or during the export, instead of returning bytes that
    /// silently miss those changes. `fork` passes `false`: it has no error to return.
    pub(crate) fn export_inner(
        &self,
        mode: ExportMode,
        reject_partial_history: bool,
    ) -> Result<Vec<u8>, LoroEncodeError> {
        self.touch_access_tick();
        self.with_barrier(|| {
            if reject_partial_history {
                self.oplog.lock().check_history_parsable()?;
            }
            let ans = match mode {
                ExportMode::Snapshot => export_fast_snapshot(self)?,
                ExportMode::Updates { from } => export_fast_updates(self, &from),
                ExportMode::UpdatesInRange { spans } => {
                    export_fast_updates_in_range(&self.oplog.lock(), spans.as_ref())
                }
                ExportMode::ShallowSnapshot(f) => export_shallow_snapshot(self, &f)?,
                ExportMode::StateOnly(f) => match f {
                    Some(f) => export_state_only_snapshot(self, &f)?,
                    None => export_state_only_snapshot(self, &self.oplog_frontiers())?,
                },
                ExportMode::SnapshotAt { version } => export_snapshot_at(self, &version)?,
            };
            if reject_partial_history {
                self.oplog.lock().check_history_parsable()?;
            }
            Ok(ans)
        })
    }

    /// Export received remote changes waiting for causal dependencies, excluding
    /// local uncommitted operations. Returns `None` when nothing remains pending.
    /// This method does not commit, consume pending, emit events, or advance the
    /// applied version/frontiers. Ordinary snapshot/update exports omit pending.
    ///
    /// Capture snapshot and sidecar without concurrent document mutations/imports
    /// and persist them as one consistent checkpoint. `None` in a new checkpoint
    /// must clear its old sidecar; errors must not be treated as `None`. Restore
    /// snapshot then sidecar with ordinary `import`; actual missing dependencies
    /// must still arrive. This method cannot supply never-received predecessors
    /// and does not define cursor advancement or whether the document is synced.
    pub fn export_pending_updates(&self) -> Result<Option<Vec<u8>>, LoroEncodeError> {
        encoding::export_pending_updates(&self.oplog.lock())
    }

    /// The doc only contains the history since the shallow history start version vector.
    ///
    /// This is empty if the doc is not shallow.
    ///
    /// The ops included by the shallow history start version vector are not in the doc.
    pub fn shallow_since_vv(&self) -> ImVersionVector {
        self.oplog().lock().shallow_since_vv().clone()
    }

    pub fn shallow_since_frontiers(&self) -> Frontiers {
        self.oplog().lock().shallow_since_frontiers().clone()
    }

    /// Check if the doc contains the full history.
    pub fn is_shallow(&self) -> bool {
        !self.oplog().lock().shallow_since_vv().is_empty()
    }

    /// Get the number of operations in the pending transaction.
    ///
    /// The pending transaction is the one that is not committed yet. It will be committed
    /// after calling `doc.commit()`, `doc.export(mode)` or `doc.checkout(version)`.
    pub fn get_pending_txn_len(&self) -> usize {
        if let Some(txn) = self.txn.lock().as_ref() {
            txn.len()
        } else {
            0
        }
    }

    /// [`Self::try_find_id_spans_between`] for a caller that cannot handle an error.
    ///
    /// # Panics
    ///
    /// Where that method returns an error, once no lock is held, so the doc stays usable.
    #[inline]
    pub fn find_id_spans_between(&self, from: &Frontiers, to: &Frontiers) -> VersionVectorDiff {
        self.try_find_id_spans_between(from, to)
            .unwrap_or_else(|err| panic!("{err}"))
    }

    /// Finds operation spans between versions, reporting unreadable history.
    pub fn try_find_id_spans_between(
        &self,
        from: &Frontiers,
        to: &Frontiers,
    ) -> LoroResult<VersionVectorDiff> {
        let oplog = self.oplog.lock();
        oplog.check_history_parsable()?;
        for id in from.iter().chain(to.iter()) {
            if !oplog.vv().includes_id(id) {
                return Err(LoroError::FrontiersNotFound(id));
            }
        }
        if from != to
            && (oplog.dag.is_before_shallow_root(from) || oplog.dag.is_before_shallow_root(to))
        {
            return Err(LoroError::SwitchToVersionBeforeShallowRoot);
        }
        oplog.dag.try_find_path(from, to)
    }

    /// Subscribe to the first commit from a peer. Operations performed on the `LoroDoc` within this callback
    /// will be merged into the current commit.
    ///
    /// This is useful for managing the relationship between `PeerID` and user information.
    /// For example, you could store user names in a `LoroMap` using `PeerID` as the key and the `UserID` as the value.
    pub fn subscribe_first_commit_from_peer(
        &self,
        callback: FirstCommitFromPeerCallback,
    ) -> Subscription {
        let (s, enable) = self
            .first_commit_from_peer_subs
            .inner()
            .insert((), callback);
        enable();
        s
    }

    /// Subscribe to the pre-commit event.
    ///
    /// The callback will be called when the changes are committed but not yet applied to the OpLog.
    /// You can modify the commit message and timestamp in the callback by [`ChangeModifier`].
    pub fn subscribe_pre_commit(&self, callback: PreCommitCallback) -> Subscription {
        let (s, enable) = self.pre_commit_subs.inner().insert((), callback);
        enable();
        s
    }
}

/// A root container touched by any op in a doc's history is part of its value, even
/// when the ops add up to an empty value: the peer that wrote them created the root's
/// state with its first local op. A replay whose net diff for the root is empty never
/// creates that state, so create it here (loro-dev/loro#1156).
///
/// `calc` must have computed diffs whose ranges all lie in the history of the state's
/// new version.
fn materialize_touched_roots(state: &mut DocState, calc: &DiffCalculator, arena: &SharedArena) {
    for idx in calc.touched_containers() {
        if let Some(id) = arena.idx_to_id(idx) {
            if id.is_root() && !id.is_mergeable() && !state.store.contains_id(&id) {
                state.ensure_container(&id);
            }
        }
    }
}

fn pending_root_containers_to_materialize(
    oplog: &OpLog,
    changes: &[Change],
) -> LoroResult<Vec<ContainerID>> {
    let mut roots = FxHashSet::default();
    for change in changes {
        if change.ctr_end() <= oplog.vv().get(&change.id.peer).copied().unwrap_or(0) {
            continue;
        }

        if oplog.dag.is_before_shallow_root(&change.deps)
            || oplog
                .dag
                .try_get_change_lamport_from_deps(&change.deps)?
                .is_some()
        {
            continue;
        }

        for op in change.ops.iter() {
            let id = oplog
                .arena
                .get_container_id(op.container)
                .expect("decoded op container should be registered");
            // Mergeable containers share the `ContainerID::Root` namespace but are logical
            // *children*: their existence is governed by their parent map's marker, and their
            // parent can be any (possibly not-yet-imported) map. Eagerly materializing one
            // while its causal dependencies are still pending has no valid parent edge to
            // resolve its depth against, which used to panic in `ContainerWrapper::new`.
            // They get materialized correctly through the normal diff path once the creating
            // change applies, so skip them here (mirrors the `!is_mergeable()` guard in
            // `ensure_root_container`). See the `mergeable_container::pending` regression test.
            if id.is_root() && !id.is_mergeable() {
                roots.insert(id);
            }
        }
    }

    Ok(roots.into_iter().collect())
}

/// Version vector of `frontiers`. `None` from the dag walk is a missing id, not
/// an empty vector and not a panic.
fn frontiers_to_vv_or_not_found(oplog: &OpLog, frontiers: &Frontiers) -> LoroResult<VersionVector> {
    match oplog.dag.try_frontiers_to_vv(frontiers)? {
        Some(vv) => Ok(vv),
        None => {
            for id in frontiers.iter() {
                if oplog.dag.try_get(id)?.is_none() {
                    return Err(LoroError::FrontiersNotFound(id));
                }
            }
            Err(LoroError::NotFoundError(
                format!("Cannot find the specified version {frontiers:?}").into_boxed_str(),
            ))
        }
    }
}

/// Identify a causally closed update component that can be applied without replaying unrelated
/// history. The first version is deliberately narrow: it accepts only scalar Map operations on
/// top-level roots that never appeared in the existing history.
fn isolated_scalar_root_batch(
    oplog: &OpLog,
    changes: &[Change],
) -> Option<(VersionVector, FxHashSet<InternalString>)> {
    if changes.is_empty() || !oplog.shallow_since_vv().is_empty() {
        return None;
    }

    // Every peer in the batch must be brand new and contiguously covered from counter 0.
    // (`changes` is sorted by lamport, so one peer's changes appear in counter order.)
    let mut component_vv = VersionVector::new();
    let mut root_names = FxHashSet::default();
    for change in changes {
        let peer = change.id.peer;
        if oplog.vv().get(&peer).copied().unwrap_or(0) != 0 {
            return None;
        }
        let end = component_vv.entry(peer).or_insert(0);
        if change.id.counter != *end {
            return None;
        }
        *end = change.ctr_end();

        for op in change.ops.iter() {
            let cid = oplog.arena.idx_to_id(op.container)?;
            if cid.is_mergeable() {
                return None;
            }
            let ContainerID::Root {
                name,
                container_type: ContainerType::Map,
            } = cid
            else {
                return None;
            };
            let InnerContent::Map(map) = &op.content else {
                return None;
            };
            if let Some(value) = &map.value {
                if value.is_container()
                    || loro_common::parse_mergeable_marker(
                        &ContainerID::new_root(&name, ContainerType::Map),
                        &map.key,
                        value,
                    )
                    .is_some()
                {
                    return None;
                }
            }
            root_names.insert(name);
        }
    }

    // The batch must be causally closed: every dependency points inside the batch itself.
    for change in changes {
        for dep in change.deps.iter() {
            if dep.counter >= component_vv.get(&dep.peer).copied().unwrap_or(0) {
                return None;
            }
        }
    }

    if oplog
        .change_store()
        .old_history_may_touch_root_names(&root_names)
    {
        return None;
    }

    Some((component_vv, root_names))
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ChangeTravelError {
    #[error(transparent)]
    HistoryUnreadable(#[from] LoroError),
    #[error("Target id not found {0:?}")]
    TargetIdNotFound(ID),
    #[error("The shallow history of the doc doesn't include the target version")]
    TargetVersionNotIncluded,
}

impl LoroDoc {
    pub fn travel_change_ancestors(
        &self,
        ids: &[ID],
        f: &mut dyn FnMut(ChangeMeta) -> ControlFlow<()>,
    ) -> Result<(), ChangeTravelError> {
        let (options, guard) = self.implicit_commit_then_stop();
        drop(guard);
        struct PendingNode(ChangeMeta);
        impl PartialEq for PendingNode {
            fn eq(&self, other: &Self) -> bool {
                self.0.lamport_last() == other.0.lamport_last() && self.0.id.peer == other.0.id.peer
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
                    .then_with(|| self.0.id.peer.cmp(&other.0.id.peer))
            }
        }

        let ans = (|| {
            for id in ids {
                let op_log = &self.oplog().lock();
                op_log.check_history_parsable()?;
                if !op_log.vv().includes_id(*id) {
                    return Err(ChangeTravelError::TargetIdNotFound(*id));
                }
                if op_log.dag.shallow_since_vv().includes_id(*id) {
                    return Err(ChangeTravelError::TargetVersionNotIncluded);
                }
            }

            let mut visited = FxHashSet::default();
            let mut pending: BinaryHeap<PendingNode> = BinaryHeap::new();
            for id in ids {
                let change = self
                    .oplog()
                    .lock()
                    .try_get_change_at(*id)?
                    .ok_or(ChangeTravelError::TargetIdNotFound(*id))?;
                pending.push(PendingNode(ChangeMeta::from_change(&change)));
            }
            while let Some(PendingNode(node)) = pending.pop() {
                let deps = node.deps.clone();
                if f(node).is_break() {
                    break;
                }

                for dep in deps.iter() {
                    let Some(dep_node) = self.oplog().lock().try_get_change_at(dep)? else {
                        continue;
                    };
                    if visited.contains(&dep_node.id) {
                        continue;
                    }

                    visited.insert(dep_node.id);
                    pending.push(PendingNode(ChangeMeta::from_change(&dep_node)));
                }
            }

            Ok(())
        })();
        self.renew_txn_if_auto_commit(options);
        ans
    }

    pub fn get_changed_containers_in(&self, id: ID, len: usize) -> FxHashSet<ContainerID> {
        self.with_barrier(|| {
            let mut set = FxHashSet::default();
            let len = i64::try_from(len).unwrap_or(i64::MAX);
            let start = i64::from(id.counter);
            let end = start.saturating_add(len);

            {
                let oplog = self.oplog().lock();
                // The part of the range this doc holds.
                let first = oplog.shallow_since_vv().get(&id.peer).copied().unwrap_or(0);
                let last = oplog.vv().get(&id.peer).copied().unwrap_or(0);
                let start = start.max(i64::from(first));
                let end = end.min(i64::from(last));
                if start >= end {
                    return set;
                }

                let span = IdSpan::new(id.peer, start as i32, end as i32);
                for op in oplog.iter_ops(span) {
                    let id = oplog.arena.get_container_id(op.container()).unwrap();
                    set.insert(id);
                }
            }
            set
        })
    }

    /// Gets modified container IDs, reporting unreadable history.
    pub fn try_get_changed_containers_in(
        &self,
        id: ID,
        len: usize,
    ) -> LoroResult<FxHashSet<ContainerID>> {
        self.oplog.lock().check_history_parsable()?;
        let containers = self.get_changed_containers_in(id, len);
        self.oplog.lock().check_history_parsable()?;
        Ok(containers)
    }

    pub fn delete_root_container(&self, cid: ContainerID) {
        if !cid.is_root() {
            return;
        }

        // Do not treat "not in arena" as non-existence; consult state/kv
        if !self.has_container(&cid) {
            return;
        }

        let Some(h) = self.get_handler(cid.clone()) else {
            return;
        };

        self.config
            .deleted_root_containers
            .lock()
            .insert(cid.clone());
        if let Err(e) = h.clear() {
            self.config.deleted_root_containers.lock().remove(&cid);
            eprintln!("Failed to clear handler: {:?}", e);
        }
    }

    pub fn set_hide_empty_root_containers(&self, hide: bool) {
        self.config
            .hide_empty_root_containers
            .store(hide, std::sync::atomic::Ordering::Relaxed);
    }
}

pub(crate) enum QueryPosError {
    Missing(CannotFindRelativePosition),
    History(LoroError),
}

impl From<LoroError> for QueryPosError {
    fn from(err: LoroError) -> Self {
        QueryPosError::History(err)
    }
}

impl From<CannotFindRelativePosition> for QueryPosError {
    fn from(err: CannotFindRelativePosition) -> Self {
        QueryPosError::Missing(err)
    }
}

fn find_last_delete_op(oplog: &OpLog, id: ID, idx: ContainerIdx) -> LoroResult<Option<ID>> {
    // Any delete op that covers `id` must have observed it, so its peer's counter
    // at delete time was > id.counter. start_vv (the vv at `id`) is therefore a
    // valid lower bound: changes at or before start_vv[peer] predate `id` and can
    // be skipped. We scan peer-by-peer rather than using the DAG-ordered
    // iter_changes_causally_rev, which is O(total changes).
    //
    // We choose the matching delete op with the greatest (op_lamport, peer, counter)
    // ordering. op_lamport is the Lamport of the specific op within the change
    // (change.lamport + op offset), not just the change's starting Lamport, so
    // concurrent deletes with equal change Lamports are broken deterministically.
    let start_vv = match oplog.dag.try_frontiers_to_vv(&id.into())? {
        Some(vv) => vv,
        None => oplog.shallow_since_vv().to_vv(),
    };

    // (op_lamport, peer) gives a deterministic total order for concurrent deletes.
    // A single peer cannot produce two ops with the same lamport, so peer suffices
    // as a tie-breaker.
    let mut best: Option<((loro_common::Lamport, loro_common::PeerID), ID)> = None;

    for change in oplog.iter_changes_peer_by_peer(&start_vv, oplog.vv()) {
        let peer = change.peer();
        for op in change.ops.iter() {
            if op.container != idx {
                continue;
            }
            if let InnerContent::List(InnerListOp::Delete(d)) = &op.content {
                if d.id_start.to_span(d.atom_len()).contains(id) {
                    debug_assert!(op.counter >= change.id().counter);
                    let op_lamport =
                        change.lamport + (op.counter - change.id().counter) as loro_common::Lamport;
                    let key = (op_lamport, peer);
                    if best.is_none_or(|(bk, _)| key > bk) {
                        best = Some((key, ID::new(peer, op.counter)));
                    }
                }
            }
        }
    }

    // The legacy peer iterator records a bad block and skips it. A skipped
    // block may hold the delete, so that is a decode error, not a missing id.
    oplog.check_history_parsable()?;
    Ok(best.map(|(_, op_id)| op_id))
}

/// Cleanup for the critical section of [`LoroDoc::import_batch`].
///
/// `import_batch` force-detaches the document so each blob only touches the `OpLog`,
/// then reattaches once at the end. Leaving that section without reattaching strands
/// the document in detached mode: further imports stop being reflected in `DocState`,
/// local edits branch off a stale version, and every later `attach()` re-runs the same
/// failing checkout. `finish` owns that reattach, and `import_batch` must call it
/// explicitly on every exit path — including after catching a panic, because cleanup
/// cannot run *while* unwinding (`MutexGuard::drop` poisons its mutex when a panic is
/// in flight, and a second panic during unwind aborts the process). A `Drop` impl
/// therefore cannot help; the debug-only one below only asserts the contract.
struct BatchImportGuard<'a> {
    doc: &'a LoroDoc,
    /// Whether the doc was attached before the batch and therefore owes a reattach.
    /// It also decides whether we own the batch-wide import rollback scope.
    was_attached: bool,
    has_doc_budget: bool,
    has_import_rollback: bool,
    has_created_journal: bool,
    pre_batch_frontiers: Frontiers,
    /// Held for the whole batch so no other thread can renew the txn and edit while
    /// we are detached. `_checkout_to_latest_without_commit` requires it to be held.
    txn: Option<LoroMutexGuard<'a, Option<Transaction>>>,
    options: Option<CommitOptions>,
    /// A full snapshot initialized a previously empty document. On checkout
    /// failure both its lazy state and its history must be reset, not just the
    /// updates recorded in the suffix journal.
    snapshot_checkpoint: Option<BatchSnapshotRollback>,
    linear_before: u64,
    #[cfg(debug_assertions)]
    finished: bool,
}

/// What [`LoroDoc::import_blobs_as_batch`] did.
struct BatchRun {
    /// `(index in the caller's input, what the import of that blob returned)`, in the
    /// order the blobs were imported.
    results: Vec<(usize, LoroResult<ImportStatus>)>,
    /// Everything the blobs imported. Empty after a rollback.
    success: VersionRange,
    /// Everything the doc waits for when the batch ends.
    pending: VersionRange,
    /// The closing checkout failed and the whole batch was rolled back.
    rollback_error: Option<LoroError>,
}

enum ImportInput<'a> {
    Slice(&'a [u8]),
    Bytes(bytes::Bytes),
}

struct BatchBlobSortKey {
    mode: encoding::EncodedBlobMode,
    change_num: u32,
}

impl From<ImportBlobMetadata> for BatchBlobSortKey {
    fn from(meta: ImportBlobMetadata) -> Self {
        Self {
            mode: meta.mode,
            change_num: meta.change_num,
        }
    }
}

impl BatchBlobSortKey {
    fn decode(input: &ImportInput<'_>, scratch: &mut BatchUpdateScratch) -> LoroResult<Self> {
        let bytes: &[u8] = match input {
            ImportInput::Slice(bytes) => bytes,
            ImportInput::Bytes(bytes) => bytes,
        };
        let parsed = parse_header_and_body(bytes, false)?;
        match parsed.mode {
            EncodeMode::FastUpdates => {
                let body = match input {
                    ImportInput::Slice(_) => bytes::Bytes::copy_from_slice(parsed.body),
                    ImportInput::Bytes(bytes) => bytes.slice_ref(parsed.body),
                };
                Ok(Self {
                    mode: encoding::EncodedBlobMode::Updates,
                    change_num: scratch.count_changes(body)?,
                })
            }
            EncodeMode::FastSnapshot => match input {
                ImportInput::Slice(_) => encoding::fast_snapshot::decode_snapshot_blob_meta(parsed),
                ImportInput::Bytes(bytes) => {
                    LoroDoc::decode_import_blob_meta_bytes(bytes.clone(), false)
                }
            }
            .map(Into::into),
            _ => Err(LoroError::ImportUnsupportedEncodingMode),
        }
    }
}

#[derive(Default)]
struct BatchUpdateScratch {
    arena: Option<SharedArena>,
    used_bytes: usize,
}

impl BatchUpdateScratch {
    fn count_changes(&mut self, body: bytes::Bytes) -> LoroResult<u32> {
        // Limit retained temporary container IDs between blobs. A large blob
        // gets a separate arena and cannot increase the lifetime of its data.
        const REUSE_BYTES: usize = 64 * 1024;
        if body.len() > REUSE_BYTES {
            return encoding::fast_snapshot::count_updates_for_batch(body, &SharedArena::new());
        }
        if self.used_bytes + body.len() > REUSE_BYTES {
            *self = Self::default();
        }
        self.used_bytes += body.len();
        let arena = self.arena.get_or_insert_with(SharedArena::new);
        encoding::fast_snapshot::count_updates_for_batch(body, arena)
    }
}

enum BatchBlobSource<'a> {
    Slice(&'a [u8]),
    Bytes(bytes::Bytes),
    Loader(Box<dyn FnOnce() -> LoroResult<bytes::Bytes> + 'a>),
}

#[inline]
fn imported_values_for_mode(json_lossy: bool) -> ImportedValues {
    if json_lossy {
        ImportedValues::JsonLossy
    } else {
        ImportedValues::Exact
    }
}

#[inline]
fn parsed_as_borrowed(parsed: &ParsedHeaderAndBodyBytes) -> ParsedHeaderAndBody<'_> {
    ParsedHeaderAndBody {
        checksum: parsed.checksum,
        checksum_body: &parsed.checksum_body,
        mode: parsed.mode,
        body: &parsed.body,
    }
}

fn non_empty(range: VersionRange) -> Option<VersionRange> {
    (!range.is_empty()).then_some(range)
}

struct BatchSnapshotRollback {
    arena: SharedArenaRollback,
    // Even an empty doc may expose empty containers acquired through getters.
    empty_containers: Vec<ContainerID>,
    pending: crate::oplog::PendingChanges,
}

/// Forward import diffs of movable lists take untouched element fields from
/// `DocState`. When a (forged) op targets an element the state no longer holds,
/// recompute the diff in Checkout mode, which resolves elements from history the
/// same way `checkout`, `import_batch` and snapshot replay do.
/// See `context/movable-list-op-validation.md`.
fn recalc_in_checkout_mode_if_needed(
    state: &mut DocState,
    oplog: &OpLog,
    old_vv: &VersionVector,
    old_frontiers: &Frontiers,
    diff: Vec<crate::event::InternalContainerDiff>,
) -> LoroResult<Vec<crate::event::InternalContainerDiff>> {
    if !state.needs_checkout_diff(&diff) {
        return Ok(diff);
    }

    Ok(DiffCalculator::new(true)
        .try_calc_diff_internal(
            oplog,
            old_vv,
            old_frontiers,
            oplog.vv(),
            oplog.dag.get_frontiers(),
            None,
        )?
        .0)
}

impl BatchImportGuard<'_> {
    /// Leave batch-import mode, reattach if the doc was attached, and renew the txn.
    ///
    /// If the accumulated changes cannot be applied to `DocState` (malformed remote ops
    /// reach state validation only here, because the blobs were imported while
    /// detached), the whole batch is rolled back out of the `OpLog` so the document
    /// stays attached and unchanged, and the state-apply error is returned. The range
    /// is what the doc still holds as pending afterwards: after a rollback, what was
    /// parked before the batch.
    fn finish(&mut self) -> (Option<LoroError>, VersionRange) {
        #[cfg(debug_assertions)]
        {
            self.finished = true;
        }
        let doc = self.doc;
        let txn = self.txn.take().expect("finish called twice");
        let options = self.options.take();

        doc.oplog.lock().batch_importing = false;

        // The txn guard must stay held across the checkout, and the renew below must
        // reuse it: dropping it first and re-locking would panic when this runs after
        // a caught panic, because releasing a `Mutex` guard while a panic is in
        // flight poisons the mutex.
        let mut checkout = Ok(());
        if self.was_attached {
            // The blobs were imported while detached inside this scope, so their
            // op references (movable-list elements, style end anchors) are validated
            // here, once per batch.
            let validation = doc.oplog.lock().validate_op_refs_in_import_scope();
            // A quota may reject the batch after the checkout changed the state;
            // record how to undo it (see `state/undo_log.rs`).
            let record_undo = self.has_doc_budget && self.snapshot_checkpoint.is_none();
            if record_undo {
                doc.state.lock().begin_import_undo();
            }
            checkout = validation.and_then(|()| {
                doc._checkout_to_latest_without_commit_with_event(
                    true,
                    "checkout".into(),
                    // The batch is a remote import: reporting it as a checkout
                    // would make the `UndoManager` clear its undo/redo stacks.
                    EventTriggerKind::Import,
                    !self.has_doc_budget,
                )
            });
            let mut undo = if record_undo {
                doc.state.lock().take_import_undo()
            } else {
                None
            };
            if checkout.is_ok() && self.has_doc_budget {
                doc.enforce_memory_watermarks_with_txn_bytes(0, false);
                if let Err(budget_err) =
                    doc.check_doc_budget_with_txn_bytes_and_linear(0, Some(self.linear_before))
                {
                    if self.snapshot_checkpoint.is_some() {
                        let _ = doc.state.lock().take_events();
                        checkout = Err(budget_err);
                    } else {
                        let mut state = doc.state.lock();
                        let undo = undo.take().expect("a budgeted batch records its undo log");
                        state.undo_import(undo, self.pre_batch_frontiers.clone());
                        if self.has_created_journal {
                            let created = state.store.take_created_journal();
                            state.store.remove_created(&created);
                            self.has_created_journal = false;
                        }
                        drop(state);
                        checkout = Err(budget_err);
                    }
                }
            }
            if let Err(e) = &checkout {
                // Validation precedes mutation: state is still at its pre-batch
                // version, or at the bootstrapped snapshot. Restore the matching
                // baseline in both state and history before leaving attached.
                tracing::warn!("import_batch cannot reattach, rolling the batch back: {e}");
                let mut oplog = doc.oplog.lock();
                let mut state = doc.state.lock();
                if let Some(checkpoint) = self.snapshot_checkpoint.take() {
                    let _ = state.take_events();
                    state.reset_to_empty_for_failed_snapshot_import();
                    oplog.reset_to_empty_for_failed_snapshot_import(checkpoint.arena, &mut state);
                    oplog.pending_changes = checkpoint.pending;
                    for id in checkpoint.empty_containers {
                        state.ensure_container(&id);
                    }
                    oplog.set_block_cache(doc.kv_block_cache.clone());
                    state.set_block_cache(doc.kv_block_cache.clone());
                } else {
                    if self.has_created_journal {
                        let created = state.store.take_created_journal();
                        state.store.remove_created(&created);
                        self.has_created_journal = false;
                    }
                    oplog.rollback_import(&mut state);
                }
                drop(state);
                drop(oplog);
                // The shared diff calculator cached ranges against the rolled-back
                // history; drop that cache instead of reusing stale entries.
                *doc.diff_calculator.lock() = DiffCalculator::new(true);
                doc.set_detached(false);
            } else {
                if self.has_created_journal {
                    let _ = doc.state.lock().store.take_created_journal();
                    self.has_created_journal = false;
                }
                doc.oplog.lock().commit_import_rollback();
                if self.has_doc_budget {
                    doc.emit_events();
                }
            }
        } else if self.has_import_rollback {
            if self.has_doc_budget {
                doc.enforce_memory_watermarks_with_txn_bytes(0, false);
                if let Err(budget_err) =
                    doc.check_doc_budget_with_txn_bytes_and_linear(0, Some(self.linear_before))
                {
                    let mut oplog = doc.oplog.lock();
                    let mut state = doc.state.lock();
                    oplog.rollback_import(&mut state);
                    drop(state);
                    drop(oplog);
                    *doc.diff_calculator.lock() = DiffCalculator::new(true);
                    checkout = Err(budget_err);
                } else {
                    doc.oplog.lock().commit_import_rollback();
                }
            } else {
                doc.oplog.lock().commit_import_rollback();
            }
        }

        doc.enforce_memory_watermarks_with_txn_bytes(0, false);
        if global_budget_flags() != 0 {
            doc.refresh_cached_doc_bytes_unlocked();
        }
        let pending = doc.oplog.lock().pending_changes.version_range();
        doc._renew_txn_if_auto_commit_with_guard(options, txn);
        (checkout.err(), pending)
    }
}

/// Debug-only tripwire: `finish` must be called explicitly (see the struct docs for
/// why `Drop` cannot do the cleanup itself). Release builds compile this out.
#[cfg(debug_assertions)]
impl Drop for BatchImportGuard<'_> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert!(
                self.finished,
                "BatchImportGuard dropped without calling finish()"
            );
        }
    }
}

#[derive(Debug)]
pub struct CommitWhenDrop<'a> {
    doc: &'a LoroDoc,
    default_options: CommitOptions,
}

impl Drop for CommitWhenDrop<'_> {
    fn drop(&mut self) {
        {
            let mut guard = self.doc.txn.lock();
            if let Some(txn) = guard.as_mut() {
                txn.set_default_options(std::mem::take(&mut self.default_options));
            };
        }

        self.doc.commit_then_renew();
    }
}

/// Options for configuring a commit operation.
#[derive(Debug, Clone)]
pub struct CommitOptions {
    /// Origin identifier for the commit event, used to track the source of changes.
    /// It doesn't persist.
    pub origin: Option<InternalString>,

    /// Whether to immediately start a new transaction after committing.
    /// Defaults to true.
    pub immediate_renew: bool,

    /// Custom timestamp for the commit in seconds since Unix epoch.
    /// If None, the current time will be used.
    pub timestamp: Option<Timestamp>,

    /// Optional commit message to attach to the changes. It will be persisted.
    pub commit_msg: Option<Arc<str>>,
}

impl CommitOptions {
    /// Creates a new CommitOptions with default values.
    pub fn new() -> Self {
        Self {
            origin: None,
            immediate_renew: true,
            timestamp: None,
            commit_msg: None,
        }
    }

    /// Sets the origin identifier for this commit.
    pub fn origin(mut self, origin: &str) -> Self {
        self.origin = Some(origin.into());
        self
    }

    /// Sets whether to immediately start a new transaction after committing.
    pub fn immediate_renew(mut self, immediate_renew: bool) -> Self {
        self.immediate_renew = immediate_renew;
        self
    }

    /// Set the timestamp of the commit.
    ///
    /// The timestamp is the number of **seconds** that have elapsed since 00:00:00 UTC on January 1, 1970.
    pub fn timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Sets a commit message to be attached to the changes.
    pub fn commit_msg(mut self, commit_msg: &str) -> Self {
        self.commit_msg = Some(commit_msg.into());
        self
    }

    /// Sets the origin identifier for this commit.
    pub fn set_origin(&mut self, origin: Option<&str>) {
        self.origin = origin.map(|x| x.into())
    }

    /// Sets the timestamp for this commit.
    pub fn set_timestamp(&mut self, timestamp: Option<Timestamp>) {
        self.timestamp = timestamp;
    }
}

impl Default for CommitOptions {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod test {
    use std::{
        panic::AssertUnwindSafe,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };

    use crate::{
        cursor::PosType,
        encoding::json_schema::json::{JsonOpContent, JsonSchema, ListOp},
        encoding::{fast_snapshot::EMPTY_MARK, EncodeMode},
        loro::ExportMode,
        version::{Frontiers, VersionVector},
        LoroDoc, ToJson, TreeParentId,
    };
    use bytes::{BufMut, Bytes};
    use loro_common::{ContainerID, ContainerType, ID};
    use loro_kv_store::{mem_store::MemKvConfig, MemKvStore};

    const XXH_SEED: u32 = u32::from_le_bytes(*b"LORO");

    fn encode_import_blob(mode: EncodeMode, body: &[u8]) -> Vec<u8> {
        let mut ans = Vec::new();
        ans.extend_from_slice(b"loro");
        ans.extend_from_slice(&[0; 16]);
        ans.extend_from_slice(&mode.to_bytes());
        ans.extend_from_slice(body);
        let checksum = xxhash_rust::xxh32::xxh32(&ans[20..], XXH_SEED);
        ans[16..20].copy_from_slice(&checksum.to_le_bytes());
        ans
    }

    fn encode_fast_snapshot_import(oplog_bytes: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.put_u32_le(oplog_bytes.len() as u32);
        body.extend_from_slice(oplog_bytes);
        body.put_u32_le(EMPTY_MARK.len() as u32);
        body.extend_from_slice(EMPTY_MARK);
        body.put_u32_le(0);
        encode_import_blob(EncodeMode::FastSnapshot, &body)
    }

    fn sstable_with_huge_meta_block_count() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"LORO");
        bytes.push(0);
        bytes.put_u32_le(10_000_000);
        bytes.put_u32_le(xxhash_rust::xxh32::xxh32(&[], XXH_SEED));
        bytes.put_u32_le(5);
        bytes
    }

    fn snapshot_oplog_with_malformed_block() -> Vec<u8> {
        let peer = 1;
        let id = ID::new(peer, 0);
        let vv = VersionVector::from_iter([(peer, 1)]);
        let frontiers = Frontiers::from_id(id);
        let mut store = MemKvStore::new(MemKvConfig::default());
        store.set(b"vv", vv.encode().into());
        store.set(b"fr", frontiers.encode().into());
        store.set(&id.to_bytes(), Bytes::from_static(&[0]));
        store.export_all().to_vec()
    }

    fn make_json_import_stress_doc(peer: u64) -> LoroDoc {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(peer).unwrap();

        let text = doc.get_text("text");
        let mut text_pos = 0;
        for i in 0..32 {
            let chunk = format!("segment-{i}-abcdefghijklmnopqrstuvwxyz;");
            text.insert_unicode(text_pos, &chunk).unwrap();
            text_pos += chunk.chars().count();
        }

        let list = doc.get_list("list");
        for i in 0..32 {
            list.insert(i, format!("item-{i}")).unwrap();
        }

        let map = doc.get_map("map");
        for i in 0..32 {
            let key = format!("key-{i}");
            map.insert(&key, format!("value-{i}")).unwrap();
        }

        let tree = doc.get_tree("tree");
        let mut parent = TreeParentId::Root;
        for i in 0..16 {
            let node = tree.create(parent).unwrap();
            let meta = tree.get_meta(node).unwrap();
            meta.insert("name", format!("node-{i}")).unwrap();
            meta.insert("payload", format!("payload-{i}-{}", "x".repeat(16)))
                .unwrap();
            parent = TreeParentId::Node(node);
        }

        doc
    }

    fn make_json_list_update_with_four_ops(peer: u64) -> (LoroDoc, JsonSchema) {
        let doc = LoroDoc::new();
        doc.set_peer_id(peer).unwrap();
        let map = doc.get_map("map");
        let list = doc.get_list("list");
        let text = doc.get_text("text");

        let mut txn = doc.txn().unwrap();
        map.insert_with_txn(&mut txn, "prefix", "map-value".into())
            .unwrap();
        list.insert_with_txn(&mut txn, 0, "seed".into()).unwrap();
        text.insert_with_txn(&mut txn, 0, "text-value", PosType::Unicode)
            .unwrap();
        list.insert_with_txn(&mut txn, 1, "tail".into()).unwrap();
        txn.commit().unwrap();

        let json = doc
            .export_json_updates(&Default::default(), &doc.oplog_vv(), false)
            .unwrap();
        assert_eq!(json.changes.len(), 1);
        assert_eq!(json.changes[0].ops.len(), 4);
        (doc, json)
    }

    fn move_last_list_insert_far_out_of_bounds(json: &mut JsonSchema) {
        let last_change = json.changes.last_mut().unwrap();
        let last_op = last_change.ops.last_mut().unwrap();
        match &mut last_op.content {
            JsonOpContent::List(ListOp::Insert { pos, .. }) => {
                *pos = 1_000;
            }
            other => panic!("expected list insert op, got {other:?}"),
        }
    }

    #[test]
    fn test_sync() {
        fn is_send_sync<T: Send + Sync>(_v: T) {}
        let loro = super::LoroDoc::new();
        is_send_sync(loro)
    }

    #[test]
    fn import_rejects_huge_sstable_meta_block_count_without_panic() {
        let bytes = encode_fast_snapshot_import(&sstable_with_huge_meta_block_count());

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| LoroDoc::new().import(&bytes)));
        assert!(result.is_ok(), "malformed import should not panic");
        assert!(result.unwrap().is_err());
    }

    #[test]
    fn import_rejects_malformed_change_block_without_panic() {
        let bytes = encode_fast_snapshot_import(&snapshot_oplog_with_malformed_block());

        let result = std::panic::catch_unwind(AssertUnwindSafe(|| LoroDoc::new().import(&bytes)));
        assert!(result.is_ok(), "malformed import should not panic");
        assert!(result.unwrap().is_err());
    }

    #[test]
    fn failed_import_rolls_back_oplog_and_arena() {
        let src = LoroDoc::new();
        src.set_peer_id(1).unwrap();
        let text = src.get_text("text");
        let mut txn = src.txn().unwrap();
        text.insert_with_txn(&mut txn, 0, "hello", PosType::Unicode)
            .unwrap();
        txn.commit().unwrap();
        let update = src.export(ExportMode::all_updates()).unwrap();

        let dst = LoroDoc::new();
        let vv_before_import = dst.oplog_vv();
        let state_before_import = dst.get_deep_value();
        let err = dst
            .import_with(&update, "__loro_fail_import_state_apply".into())
            .unwrap_err();
        assert!(err.to_string().contains("state apply failpoint"));
        assert_eq!(dst.oplog_vv(), vv_before_import);
        assert_eq!(dst.get_deep_value(), state_before_import);
        assert!(dst.oplog().lock().is_empty());

        dst.import(&update).unwrap();
        assert_eq!(dst.get_deep_value(), src.get_deep_value());
    }

    #[test]
    fn failed_incremental_import_restores_previous_change_store_block() {
        let src = LoroDoc::new();
        src.set_peer_id(1).unwrap();
        let text = src.get_text("text");
        let mut txn = src.txn().unwrap();
        text.insert_with_txn(&mut txn, 0, "a", PosType::Unicode)
            .unwrap();
        txn.commit().unwrap();
        let first_update = src.export(ExportMode::all_updates()).unwrap();
        let first_vv = src.oplog_vv();

        let mut txn = src.txn().unwrap();
        text.insert_with_txn(&mut txn, 1, "b", PosType::Unicode)
            .unwrap();
        txn.commit().unwrap();
        let second_update = src.export(ExportMode::updates(&first_vv)).unwrap();

        let dst = LoroDoc::new();
        dst.import(&first_update).unwrap();
        let vv_before_import = dst.oplog_vv();
        let state_before_import = dst.get_deep_value();
        dst.import_with(&second_update, "__loro_fail_import_state_apply".into())
            .unwrap_err();
        assert_eq!(dst.oplog_vv(), vv_before_import);
        assert_eq!(dst.get_deep_value(), state_before_import);

        dst.import(&second_update).unwrap();
        assert_eq!(dst.get_deep_value(), src.get_deep_value());
    }

    #[test]
    fn cross_peer_failpoint_rolls_back_before_any_write() {
        let a = LoroDoc::new_auto_commit();
        a.set_peer_id(1).unwrap();
        a.get_text("text")
            .insert(0, "hello", PosType::Unicode)
            .unwrap();
        a.commit_then_renew();
        let base = a.export(ExportMode::all_updates()).unwrap();

        let b = LoroDoc::new_auto_commit();
        b.set_peer_id(2).unwrap();
        b.import(&base).unwrap();
        let vv = b.oplog_vv();
        b.get_text("text")
            .insert(5, " world", PosType::Unicode)
            .unwrap();
        b.commit_then_renew();
        let extra = b.export(ExportMode::updates(&vv)).unwrap();

        let attached = LoroDoc::new();
        attached.import(&base).unwrap();
        let before = attached.get_deep_value();
        let vv_before = attached.oplog_vv();
        let frontiers = attached.oplog_frontiers();
        let err = attached
            .import_with(&extra, "__loro_fail_import_state_apply".into())
            .unwrap_err();
        assert!(err.to_string().contains("state apply failpoint"), "{err:?}");
        assert_eq!(attached.get_deep_value(), before);
        assert_eq!(attached.oplog_vv(), vv_before);
        assert_eq!(attached.oplog_frontiers(), frontiers);
        assert_eq!(attached.state_frontiers(), frontiers);

        attached.import(&extra).unwrap();
        assert_eq!(attached.get_text("text").to_string(), "hello world");
    }

    #[test]
    fn checkout_then_merged_local_text_insert_then_concurrent_import_matches_oracle() {
        let p1 = LoroDoc::new_auto_commit();
        p1.set_peer_id(1).unwrap();
        p1.get_text("t")
            .insert(0, "abcdef", PosType::Unicode)
            .unwrap();
        p1.commit_then_renew();
        let base = p1.export(ExportMode::all_updates()).unwrap();
        let p1_vv = p1.oplog_vv();

        let p3 = LoroDoc::new_auto_commit();
        p3.set_peer_id(3).unwrap();
        p3.import(&base).unwrap();
        p3.get_text("t").insert(0, "é", PosType::Unicode).unwrap();
        p3.commit_then_renew();

        // Warm the persistent diff calculator and then leave an RLE-merged
        // local edit right after the tracker's version before importing.
        p3.checkout(&Frontiers::from_id(ID::new(1, 2))).unwrap();
        p3.checkout_to_latest();
        p3.get_text("t").insert(1, "xyz", PosType::Unicode).unwrap();
        p3.commit_then_renew();

        p1.get_text("t").delete(1, 3, PosType::Unicode).unwrap();
        p1.get_text("t").insert(2, "😀", PosType::Unicode).unwrap();
        p1.commit_then_renew();
        let concurrent = p1.export(ExportMode::updates(&p1_vv)).unwrap();

        p3.import(&concurrent).unwrap();

        let oracle = LoroDoc::new();
        oracle
            .import(&p3.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        assert_eq!(
            p3.get_text("t").to_string(),
            oracle.get_text("t").to_string()
        );
        assert_eq!(p3.get_deep_value(), oracle.get_deep_value());
    }

    #[test]
    fn failed_import_json_updates_rolls_back_complex_empty_doc() {
        let src = make_json_import_stress_doc(11);
        let json = src
            .export_json_updates(&Default::default(), &src.oplog_vv(), false)
            .unwrap();

        let dst = LoroDoc::new();
        let vv_before_import = dst.oplog_vv();
        let frontiers_before_import = dst.oplog_frontiers();
        let state_before_import = dst.get_deep_value();
        for _ in 0..3 {
            crate::state::fail_next_import_state_apply_for_test();
            let err = dst.import_json_updates(json.clone()).unwrap_err();
            assert!(err.to_string().contains("state apply failpoint"));
            assert_eq!(dst.oplog_vv(), vv_before_import);
            assert_eq!(dst.oplog_frontiers(), frontiers_before_import);
            assert_eq!(dst.get_deep_value(), state_before_import);
            assert!(dst.oplog().lock().is_empty());
        }

        dst.import_json_updates(json).unwrap();
        assert_eq!(dst.oplog_vv(), src.oplog_vv());
        assert_eq!(dst.oplog_frontiers(), src.oplog_frontiers());
        assert_eq!(dst.get_deep_value(), src.get_deep_value());
    }

    #[test]
    fn failed_incremental_import_json_updates_restores_previous_change_store_block() {
        let src = LoroDoc::new_auto_commit();
        src.set_peer_id(12).unwrap();
        let text = src.get_text("text");
        text.insert_unicode(0, "a").unwrap();
        let list = src.get_list("list");
        list.push("seed").unwrap();
        let map = src.get_map("map");
        map.insert("seed", "value").unwrap();
        let tree = src.get_tree("tree");
        let root = tree.create(TreeParentId::Root).unwrap();
        tree.get_meta(root).unwrap().insert("name", "root").unwrap();

        let first_vv = src.oplog_vv();
        let first_json = src
            .export_json_updates(&Default::default(), &first_vv, false)
            .unwrap();

        let mut text_pos = text.len_unicode();
        for i in 0..64 {
            let chunk = format!("chunk-{i};");
            text.insert_unicode(text_pos, &chunk).unwrap();
            text_pos += chunk.chars().count();
        }
        for i in 0..32 {
            list.push(format!("after-{i}")).unwrap();
            let key = format!("after-{i}");
            map.insert(&key, format!("value-{i}")).unwrap();
        }
        let child = tree.create(TreeParentId::Node(root)).unwrap();
        tree.get_meta(child)
            .unwrap()
            .insert("name", "child")
            .unwrap();

        let second_json = src
            .export_json_updates(&first_vv, &src.oplog_vv(), false)
            .unwrap();

        let dst = LoroDoc::new();
        dst.import_json_updates(first_json).unwrap();
        let vv_before_import = dst.oplog_vv();
        let frontiers_before_import = dst.oplog_frontiers();
        let state_before_import = dst.get_deep_value();

        for _ in 0..2 {
            crate::state::fail_next_import_state_apply_for_test();
            let err = dst.import_json_updates(second_json.clone()).unwrap_err();
            assert!(err.to_string().contains("state apply failpoint"));
            assert_eq!(dst.oplog_vv(), vv_before_import);
            assert_eq!(dst.oplog_frontiers(), frontiers_before_import);
            assert_eq!(dst.get_deep_value(), state_before_import);
        }

        dst.import_json_updates(second_json).unwrap();
        assert_eq!(dst.oplog_vv(), src.oplog_vv());
        assert_eq!(dst.oplog_frontiers(), src.oplog_frontiers());
        assert_eq!(dst.get_deep_value(), src.get_deep_value());
    }

    #[test]
    fn malformed_later_import_json_update_rolls_back_after_valid_prefix_enters_oplog() {
        let peer = 13;
        let (src, good_json) = make_json_list_update_with_four_ops(peer);
        let mut bad_json = good_json.clone();
        move_last_list_insert_far_out_of_bounds(&mut bad_json);

        let good_dst = LoroDoc::new();
        good_dst.import_json_updates(good_json.clone()).unwrap();
        assert_eq!(good_dst.get_deep_value(), src.get_deep_value());

        let last_op_counter = good_json.changes[0].ops.last().unwrap().counter;
        let prefix_vv = VersionVector::from_iter([(peer, last_op_counter)]);
        let prefix_json = src
            .export_json_updates(&Default::default(), &prefix_vv, false)
            .unwrap();
        assert_eq!(
            prefix_json.changes[0].ops.len(),
            good_json.changes[0].ops.len() - 1
        );
        let good_suffix_json = src
            .export_json_updates(&prefix_vv, &src.oplog_vv(), false)
            .unwrap();
        assert_eq!(good_suffix_json.changes[0].ops.len(), 1);
        let mut bad_suffix_json = good_suffix_json.clone();
        move_last_list_insert_far_out_of_bounds(&mut bad_suffix_json);

        let prefix_dst = LoroDoc::new();
        prefix_dst.import_json_updates(prefix_json.clone()).unwrap();
        let vv_before_bad_suffix = prefix_dst.oplog_vv();
        let frontiers_before_bad_suffix = prefix_dst.oplog_frontiers();
        let state_before_bad_suffix = prefix_dst.get_deep_value();

        let bad_suffix_json = serde_json::to_string(&bad_suffix_json).unwrap();
        let err = prefix_dst
            .import_json_updates(&bad_suffix_json)
            .unwrap_err();
        assert!(
            err.to_string().contains("list diff"),
            "expected state list bounds validation, got {err:?}"
        );
        assert_eq!(prefix_dst.oplog_vv(), vv_before_bad_suffix);
        assert_eq!(prefix_dst.oplog_frontiers(), frontiers_before_bad_suffix);
        assert_eq!(prefix_dst.get_deep_value(), state_before_bad_suffix);

        prefix_dst.import_json_updates(good_suffix_json).unwrap();
        assert_eq!(prefix_dst.get_deep_value(), src.get_deep_value());
        assert_eq!(prefix_dst.oplog_vv(), src.oplog_vv());

        let dst = LoroDoc::new();
        let vv_before_import = dst.oplog_vv();
        let frontiers_before_import = dst.oplog_frontiers();
        let state_before_import = dst.get_deep_value();
        let bad_json = serde_json::to_string(&bad_json).unwrap();
        let err = dst.import_json_updates(&bad_json).unwrap_err();
        assert!(
            err.to_string().contains("list diff"),
            "expected state list bounds validation, got {err:?}"
        );
        assert_eq!(dst.oplog_vv(), vv_before_import);
        assert_eq!(dst.oplog_frontiers(), frontiers_before_import);
        assert_eq!(dst.get_deep_value(), state_before_import);
        assert!(dst.oplog().lock().is_empty());
    }

    #[test]
    fn failed_import_restores_pending_changes_that_were_applied_during_import() {
        let src = LoroDoc::new();
        src.set_peer_id(14).unwrap();
        let text = src.get_text("text");

        let mut txn = src.txn().unwrap();
        text.insert_with_txn(&mut txn, 0, "a", PosType::Unicode)
            .unwrap();
        txn.commit().unwrap();
        let first_update = src.export(ExportMode::all_updates()).unwrap();
        let first_vv = src.oplog_vv();

        let mut txn = src.txn().unwrap();
        text.insert_with_txn(&mut txn, 1, "b", PosType::Unicode)
            .unwrap();
        txn.commit().unwrap();
        let second_update = src.export(ExportMode::updates(&first_vv)).unwrap();

        let dst = LoroDoc::new();
        let status = dst.import(&second_update).unwrap();
        assert!(status.success.is_empty());
        assert!(status.pending.is_some());
        let vv_before_dependency = dst.oplog_vv();
        let frontiers_before_dependency = dst.oplog_frontiers();
        let state_before_dependency = dst.get_deep_value();

        crate::state::fail_next_import_state_apply_for_test();
        let err = dst.import(&first_update).unwrap_err();
        assert!(err.to_string().contains("state apply failpoint"));
        assert_eq!(dst.oplog_vv(), vv_before_dependency);
        assert_eq!(dst.oplog_frontiers(), frontiers_before_dependency);
        assert_eq!(dst.get_deep_value(), state_before_dependency);

        dst.import(&first_update).unwrap();
        assert_eq!(dst.oplog_vv(), src.oplog_vv());
        assert_eq!(dst.oplog_frontiers(), src.oplog_frontiers());
        assert_eq!(dst.get_deep_value(), src.get_deep_value());
    }

    #[test]
    fn failed_import_json_updates_does_not_emit_or_leave_events() {
        let (src, good_json) = make_json_list_update_with_four_ops(15);
        let mut bad_json = good_json.clone();
        move_last_list_insert_far_out_of_bounds(&mut bad_json);

        let dst = LoroDoc::new();
        let event_count = Arc::new(AtomicUsize::new(0));
        let event_count_cloned = event_count.clone();
        let _sub = dst.subscribe_root(Arc::new(move |_| {
            event_count_cloned.fetch_add(1, Ordering::SeqCst);
        }));

        let bad_json = serde_json::to_string(&bad_json).unwrap();
        let err = dst.import_json_updates(&bad_json).unwrap_err();
        assert!(
            err.to_string().contains("list diff"),
            "expected state list bounds validation, got {err:?}"
        );
        assert_eq!(event_count.load(Ordering::SeqCst), 0);
        assert!(dst.drop_pending_events().is_empty());
        assert!(dst.oplog().lock().is_empty());

        dst.import_json_updates(good_json).unwrap();
        assert_eq!(event_count.load(Ordering::SeqCst), 1);
        assert_eq!(dst.get_deep_value(), src.get_deep_value());
    }

    #[test]
    fn test_checkout() {
        let loro = LoroDoc::new();
        loro.set_peer_id(1).unwrap();
        let text = loro.get_text("text");
        let map = loro.get_map("map");
        let list = loro.get_list("list");
        let mut txn = loro.txn().unwrap();
        for i in 0..10 {
            map.insert_with_txn(&mut txn, "key", i.into()).unwrap();
            text.insert_with_txn(&mut txn, 0, &i.to_string(), PosType::Unicode)
                .unwrap();
            list.insert_with_txn(&mut txn, 0, i.into()).unwrap();
        }
        txn.commit().unwrap();
        let b = LoroDoc::new();
        b.import(&loro.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        loro.checkout(&Frontiers::default()).unwrap();
        {
            let json = &loro.get_deep_value();
            assert_eq!(
                json.to_json_value(),
                serde_json::json!({"text":"","list":[],"map":{}})
            );
        }

        b.checkout(&ID::new(1, 2).into()).unwrap();
        {
            let json = &b.get_deep_value();
            assert_eq!(
                json.to_json_value(),
                serde_json::json!({"text":"0","list":[0],"map":{"key":0}})
            );
        }

        loro.checkout(&ID::new(1, 3).into()).unwrap();
        {
            let json = &loro.get_deep_value();
            assert_eq!(
                json.to_json_value(),
                serde_json::json!({"text":"0","list":[0],"map":{"key":1}})
            );
        }

        b.checkout(&ID::new(1, 29).into()).unwrap();
        {
            let json = &b.get_deep_value();
            assert_eq!(
                json.to_json_value(),
                serde_json::json!({"text":"9876543210","list":[9,8,7,6,5,4,3,2,1,0],"map":{"key":9}})
            );
        }
    }

    #[test]
    fn import_batch_err_181() {
        let a = LoroDoc::new_auto_commit();
        let update_a = a.export(ExportMode::Snapshot);
        let b = LoroDoc::new_auto_commit();
        b.import_batch(&[update_a.unwrap()]).unwrap();
        b.get_text("text")
            .insert(0, "hello", PosType::Unicode)
            .unwrap();
        b.commit_then_renew();
        let oplog = b.oplog().lock();
        drop(oplog);
        b.export(ExportMode::all_updates()).unwrap();
    }

    #[test]
    fn poisoned_mutex_keeps_follow_up_operations_failed() {
        let doc = LoroDoc::new();
        let oplog = doc.oplog.clone();
        let _ = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _guard = oplog.lock();
            panic!("poison oplog");
        }));

        let err = std::panic::catch_unwind(AssertUnwindSafe(|| doc.oplog_vv()))
            .expect_err("poisoned lock should continue to fail fast");
        let msg = if let Some(msg) = err.downcast_ref::<&str>() {
            (*msg).to_string()
        } else if let Some(msg) = err.downcast_ref::<String>() {
            msg.clone()
        } else {
            String::new()
        };
        assert!(msg.contains("poisoned LoroMutex"), "{msg}");
    }

    #[test]
    fn repeated_independent_scalar_root_imports_scan_history_once() {
        let base = LoroDoc::new_auto_commit();
        base.set_peer_id(1).unwrap();
        for i in 0..64 {
            base.get_map("existing")
                .insert(&format!("key-{i}"), i)
                .unwrap();
            base.commit_then_renew();
        }

        let target = LoroDoc::new();
        target
            .import(&base.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        assert!(!target.has_history_cache());

        for i in 0_u64..8 {
            let remote = LoroDoc::new_auto_commit();
            remote.set_peer_id(2 + i).unwrap();
            remote
                .get_map(format!("isolated-{i}"))
                .insert("value", i as i32)
                .unwrap();
            let update = remote.export(ExportMode::all_updates()).unwrap();
            target.import(&update).unwrap();
            assert_eq!(
                target.get_map(format!("isolated-{i}")).get("value"),
                Some((i as i32).into())
            );
        }

        assert!(!target.has_history_cache());
        assert_eq!(
            target
                .oplog
                .lock()
                .change_store()
                .root_history_scan_count_for_test(),
            1
        );
    }

    #[test]
    fn independent_fast_path_rejects_root_present_only_in_snapshot_state() {
        fn snapshot_sections(doc: &LoroDoc) -> crate::encoding::fast_snapshot::Snapshot {
            let (_, txn_guard) = doc.implicit_commit_then_stop();
            drop(txn_guard);
            crate::encoding::fast_snapshot::encode_snapshot_inner(doc).unwrap()
        }

        let base = LoroDoc::new_auto_commit();
        base.set_peer_id(1).unwrap();
        base.get_map("existing").insert("value", "base").unwrap();
        let base_sections = snapshot_sections(&base);

        // The donor's value must lose the register comparison against the real
        // ops below (same lamport, lower peer), so both the general path and a
        // wrongly-taken fast path would end with "winner"; the fast-path
        // counter is what proves the rejection.
        let state_donor = LoroDoc::new_auto_commit();
        state_donor.set_peer_id(0).unwrap();
        state_donor
            .get_map("isolated")
            .insert("value", "stale")
            .unwrap();
        let donor_sections = snapshot_sections(&state_donor);

        let target = LoroDoc::new();
        crate::encoding::fast_snapshot::decode_snapshot_inner(
            crate::encoding::fast_snapshot::Snapshot {
                oplog_bytes: base_sections.oplog_bytes,
                state_bytes: donor_sections.state_bytes,
                shallow_root_state_bytes: Bytes::new(),
            },
            &target,
            Default::default(),
        )
        .unwrap();
        assert!(!target.has_history_cache());

        let left = LoroDoc::new_auto_commit();
        left.set_peer_id(2).unwrap();
        left.get_map("isolated").insert("value", "left").unwrap();
        let right = LoroDoc::new_auto_commit();
        right.set_peer_id(3).unwrap();
        right.get_map("isolated").insert("value", "winner").unwrap();
        let aggregate = LoroDoc::new();
        aggregate
            .import(&left.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        aggregate
            .import(&right.export(ExportMode::all_updates()).unwrap())
            .unwrap();

        let hits = super::isolated_fast_path_hits_for_test();
        target
            .import(&aggregate.export(ExportMode::all_updates()).unwrap())
            .unwrap();

        assert_eq!(
            target.get_map("isolated").get("value"),
            Some("winner".into())
        );
        assert_eq!(super::isolated_fast_path_hits_for_test(), hits);
    }

    #[test]
    fn independent_scalar_root_import_rolls_back_after_state_failure() {
        let base = LoroDoc::new_auto_commit();
        base.set_peer_id(1).unwrap();
        base.get_map("existing").insert("value", "base").unwrap();

        let target = LoroDoc::new();
        target
            .import(&base.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        let vv_before = target.oplog_vv();
        let state_before = target.get_deep_value();

        let remote = LoroDoc::new_auto_commit();
        remote.set_peer_id(2).unwrap();
        remote.get_map("isolated").insert("value", 7).unwrap();
        let update = remote.export(ExportMode::all_updates()).unwrap();

        crate::state::fail_next_import_state_apply_for_test();
        let err = target.import(&update).unwrap_err();
        assert!(err.to_string().contains("state apply failpoint"));
        assert_eq!(target.oplog_vv(), vv_before);
        assert_eq!(target.get_deep_value(), state_before);
        assert!(!target.has_history_cache());

        target.import(&update).unwrap();
        assert_eq!(target.get_map("isolated").get("value"), Some(7.into()));
    }

    #[test]
    fn independent_fast_path_rejects_root_name_seen_in_history() {
        let base = LoroDoc::new_auto_commit();
        base.set_peer_id(1).unwrap();
        base.get_map("shared").insert("value", "base").unwrap();

        let target = LoroDoc::new();
        target
            .import(&base.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        assert!(!target.has_history_cache());

        let remote = LoroDoc::new_auto_commit();
        remote.set_peer_id(999).unwrap();
        remote.get_map("shared").insert("value", "remote").unwrap();
        let hits = super::isolated_fast_path_hits_for_test();
        target
            .import(&remote.export(ExportMode::all_updates()).unwrap())
            .unwrap();

        assert_eq!(target.get_map("shared").get("value"), Some("remote".into()));
        assert_eq!(super::isolated_fast_path_hits_for_test(), hits);
    }

    #[test]
    fn independent_fast_path_rejects_deleted_root_name() {
        let base = LoroDoc::new_auto_commit();
        base.set_peer_id(1).unwrap();
        let root_id = ContainerID::new_root("deleted", ContainerType::Map);
        base.get_map("deleted").insert("value", "base").unwrap();
        base.delete_root_container(root_id);

        let target = LoroDoc::new();
        target
            .import(&base.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        assert!(!target.has_history_cache());

        let remote = LoroDoc::new_auto_commit();
        remote.set_peer_id(2).unwrap();
        remote.get_map("deleted").insert("value", "remote").unwrap();
        let hits = super::isolated_fast_path_hits_for_test();
        target
            .import(&remote.export(ExportMode::all_updates()).unwrap())
            .unwrap();

        assert_eq!(super::isolated_fast_path_hits_for_test(), hits);
        // The root's clearing ops outrank the remote write, so the map stays
        // empty; `deleted_root_containers` only affects root visibility, so
        // compare the container value with a doc that saw the other order.
        let reference = LoroDoc::new();
        reference
            .import(&remote.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        reference
            .import(&base.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        use crate::handler::HandlerTrait;
        assert_eq!(
            target.get_map("deleted").get_deep_value(),
            reference.get_map("deleted").get_deep_value()
        );
        assert!(target.get_map("deleted").is_empty());
    }
}

const BUDGET_FLAG_MAX_DOC: u8 = 1 << 0;
const BUDGET_FLAG_WATERMARKS: u8 = 1 << 1;
const BUDGET_FLAG_MAX_IMPORT_BLOB: u8 = 1 << 2;

static GLOBAL_BUDGET_FLAGS: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
static NEXT_POOL_DOC_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static GLOBAL_ACCESS_TICK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static GLOBAL_POOL: std::sync::OnceLock<GlobalMemoryPool> = std::sync::OnceLock::new();

/// Whether a document's cached statistics (`cached_doc_bytes`) may be stale.
///
/// The native global estimate (no allocator tracker) used to recompute every live
/// document's statistics on each budgeted commit, so a commit cost grew with the
/// number of documents. Now it sums the cached values and recomputes only
/// documents marked here ([`GlobalMemoryPool::refresh_dirty_docs`]).
///
/// Marking rule: whatever can change a component of [`DocMemoryStats`] marks the
/// document while it holds the lock that component is read under, or after the
/// change. The refresh clears the flag after taking the document's locks and before
/// reading the components, so a change is either seen by that read or marks again.
/// Marks come from every acquisition of the document's `Txn`, `OpLog`, `DocState`
/// and `DiffCalculator` locks (`LoroMutex::set_stats_dirty`), arena writes
/// (`SharedArena::mark_stats_dirty`; handlers register containers without the
/// document's locks), change blocks parsed by the creator resolver, and undo
/// managers (`MarkedUndoGuard`). A flag going from clean to dirty adds the id to
/// `GlobalMemoryPool::dirty_docs`, also without a global budget (the statistics
/// APIs work without one); a document starts dirty and queued. The set holds live
/// documents only (`unregister` removes the id), and a document that is never
/// refreshed is added once.
#[derive(Debug)]
pub(crate) struct DocStatsDirty {
    doc_id: u64,
    dirty: std::sync::atomic::AtomicBool,
}

impl DocStatsDirty {
    fn new(doc_id: u64) -> Self {
        Self {
            doc_id,
            // Queued by `GlobalMemoryPool::register`.
            dirty: std::sync::atomic::AtomicBool::new(true),
        }
    }

    /// Marks the statistics stale. Cheap when already dirty (one relaxed load); a
    /// stale `true` read cannot skip a needed push, since the refresh that cleared
    /// it held the lock the marker holds now, or ran before the change it marks.
    #[inline]
    pub(crate) fn mark(&self) {
        if !self.dirty.load(Relaxed) {
            self.mark_slow();
        }
    }

    #[cold]
    fn mark_slow(&self) {
        if self.doc_id == 0 || self.dirty.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        if let Some(pool) = GLOBAL_POOL.get() {
            pool.push_dirty(self.doc_id);
        }
    }

    fn is_dirty(&self) -> bool {
        self.dirty.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[inline]
pub(crate) fn global_budget_flags() -> u8 {
    GLOBAL_BUDGET_FLAGS.load(Acquire)
}

#[inline]
fn global_watermarks_active() -> bool {
    global_budget_flags() & BUDGET_FLAG_WATERMARKS != 0
}

fn compute_global_flags(budget: &GlobalMemoryBudget) -> u8 {
    let mut flags = 0u8;
    let has_wasm_limit = cfg!(target_arch = "wasm32") && budget.max_wasm_linear_bytes.is_some();
    if budget.max_heap_bytes.is_some() || has_wasm_limit {
        flags |= BUDGET_FLAG_MAX_DOC | BUDGET_FLAG_MAX_IMPORT_BLOB;
    }
    if budget.effective_high_watermark().is_some()
        || budget.max_kv_block_cache_bytes.is_some()
        || has_wasm_limit
    {
        flags |= BUDGET_FLAG_WATERMARKS;
    }
    if budget.max_import_blob_bytes.is_some() {
        flags |= BUDGET_FLAG_MAX_IMPORT_BLOB;
    }
    flags
}

fn make_global_budget_err(used: u64, limit: u64) -> LoroError {
    let limit_usize = usize::try_from(limit)
        .unwrap_or(usize::MAX - 1)
        .min(usize::MAX - 1);
    let used_usize = usize::try_from(used)
        .unwrap_or(usize::MAX)
        .max(limit_usize.saturating_add(1));
    LoroError::memory_limit_exceeded(MemoryLimitKind::GlobalBudget, used_usize, limit_usize)
}

/// Registered documents in the global memory pool.
type RegistryEntries = FxHashMap<u64, std::sync::Weak<LoroDocInner>>;

struct GlobalMemoryPool {
    budget: parking_lot::Mutex<GlobalMemoryBudget>,
    docs: parking_lot::Mutex<RegistryEntries>,
    total_cached_doc_bytes: std::sync::atomic::AtomicU64,
    /// Sum of `cached_spilled_bytes`.
    total_cached_spilled_bytes: std::sync::atomic::AtomicU64,
    /// Live documents whose cached statistics may be stale; see [`DocStatsDirty`].
    dirty_docs: parking_lot::Mutex<FxHashSet<u64>>,
    global_block_cache: SharedBlockCache,
    /// See [`GlobalMemoryPool::soft_reclaim_backed_off`].
    soft_backoff: SoftBackoff,
}

/// Skips soft sweeps that recently failed to reach their target: one global
/// (`GlobalMemoryPool::soft_backoff`) and one per document
/// (`LoroDocInner::soft_backoff`, used by `enforce_watermarks_with_budget_locked`).
#[derive(Debug)]
pub(crate) struct SoftBackoff {
    /// Usage left by the last sweep that could not reach its target, or 0.
    floor: std::sync::atomic::AtomicU64,
    /// Sweeps skipped since `floor` was set.
    skips: std::sync::atomic::AtomicU32,
}

impl SoftBackoff {
    pub(crate) const fn new() -> Self {
        Self {
            floor: std::sync::atomic::AtomicU64::new(0),
            skips: std::sync::atomic::AtomicU32::new(0),
        }
    }

    /// Whether a sweep should be skipped for usage `used` above the high
    /// watermark: the previous sweep could not get below its target, usage has
    /// not grown meaningfully since, and fewer than [`SOFT_BACKOFF_MAX_SKIPS`]
    /// checks were skipped.
    fn backed_off(&self, used: u64) -> bool {
        let floor = self.floor.load(Relaxed);
        if floor == 0 {
            return false;
        }
        let growth = (floor / SOFT_BACKOFF_GROWTH_DIVISOR).max(SOFT_BACKOFF_MIN_GROWTH);
        used <= floor.saturating_add(growth)
            && self.skips.fetch_add(1, Relaxed) < SOFT_BACKOFF_MAX_SKIPS
    }

    fn record(&self, reached_target: bool, used: u64) {
        if reached_target {
            self.reset();
        } else {
            self.skips.store(0, Relaxed);
            self.floor.store(used.max(1), Relaxed);
        }
    }

    pub(crate) fn reset(&self) {
        if self.floor.load(Relaxed) != 0 {
            self.floor.store(0, Relaxed);
        }
    }
}

/// A failed soft sweep is retried once usage grows by this fraction of what it
/// left (at least [`SOFT_BACKOFF_MIN_GROWTH`]) ...
const SOFT_BACKOFF_GROWTH_DIVISOR: u64 = 8;
const SOFT_BACKOFF_MIN_GROWTH: u64 = 256 * 1024;
/// ... or after this many skipped checks, so documents that became idle in the
/// meantime are still reclaimed without growth.
const SOFT_BACKOFF_MAX_SKIPS: u32 = 64;

impl GlobalMemoryPool {
    fn new() -> Self {
        Self {
            budget: parking_lot::Mutex::new(GlobalMemoryBudget::default()),
            docs: parking_lot::Mutex::new(FxHashMap::default()),
            total_cached_doc_bytes: std::sync::atomic::AtomicU64::new(0),
            total_cached_spilled_bytes: std::sync::atomic::AtomicU64::new(0),
            dirty_docs: parking_lot::Mutex::new(FxHashSet::default()),
            global_block_cache: SharedBlockCache::new(DEFAULT_GLOBAL_BLOCK_CACHE_BYTES),
            soft_backoff: SoftBackoff::new(),
        }
    }

    /// Whether the soft sweep should be skipped for usage `used` above the high
    /// watermark: the previous sweep could not get below its target, usage has
    /// not grown meaningfully since, and fewer than [`SOFT_BACKOFF_MAX_SKIPS`]
    /// checks were skipped. Each sweep runs Stage 1 (including a full KV flush
    /// and arena copy) on the active document, so repeating one that cannot
    /// succeed made every commit pay for it. Hard admission does not use this.
    fn soft_reclaim_backed_off(&self, used: u64) -> bool {
        self.soft_backoff.backed_off(used)
    }

    fn record_soft_sweep_result(&self, reached_target: bool, used: u64) {
        self.soft_backoff.record(reached_target, used);
    }

    fn register(&self, id: u64, weak: std::sync::Weak<LoroDocInner>) {
        self.docs.lock().insert(id, weak);
        self.push_dirty(id);
    }

    fn unregister(&self, id: u64, last_cached_bytes: usize, last_spilled_bytes: usize) {
        self.docs.lock().remove(&id);
        self.dirty_docs.lock().remove(&id);
        sub_saturating(&self.total_cached_doc_bytes, last_cached_bytes);
        sub_saturating(&self.total_cached_spilled_bytes, last_spilled_bytes);
    }

    fn live_doc_count(&self) -> usize {
        self.docs
            .lock()
            .values()
            .filter(|weak| weak.strong_count() > 0)
            .count()
    }

    fn push_dirty(&self, id: u64) {
        self.dirty_docs.lock().insert(id);
    }

    /// Recomputes the statistics of the documents marked dirty, except `active_id`,
    /// which stays queued: its caller supplies its bytes and keeps changing it.
    /// A document that is busy (or whose undo manager is) keeps its cached value and
    /// stays queued, as the full scan used to keep a busy document's cached value.
    fn refresh_dirty_docs(&self, active_id: Option<u64>) {
        let ids = std::mem::take(&mut *self.dirty_docs.lock());
        if ids.is_empty() {
            return;
        }
        let mut requeue = Vec::new();
        let targets: Vec<_> = {
            let docs = self.docs.lock();
            ids.into_iter()
                .filter_map(|id| {
                    if active_id == Some(id) {
                        requeue.push(id);
                        return None;
                    }
                    docs.get(&id).map(|weak| (id, weak.clone()))
                })
                .collect()
        };
        for (id, weak) in targets {
            #[cfg(test)]
            DIRTY_REFRESHES.with(|n| n.set(n.get() + 1));
            // A document that cannot be upgraded is being dropped and unregisters.
            if let Some(doc) = RegistryRef::upgrade(&weak) {
                if !refresh_cached_doc_bytes_for_inner(&doc) {
                    requeue.push(id);
                }
            }
        }
        if !requeue.is_empty() {
            self.dirty_docs.lock().extend(requeue);
        }
    }

    /// The estimated bytes of all documents plus the global block cache, and their
    /// spilled bytes, with `active`'s bytes as given. Costs the number of documents
    /// changed since the last call, not the number of documents.
    fn estimated_docs_total(&self, active: Option<(&LoroDocInner, usize)>) -> (u64, u64) {
        self.refresh_dirty_docs(active.map(|(doc, _)| doc.pool_doc_id));
        if let Some((doc, bytes)) = active {
            self.update_cached_doc_bytes(doc, bytes, None);
        }
        #[cfg(debug_assertions)]
        self.debug_assert_clean_caches_fresh(active.map(|(doc, _)| doc.pool_doc_id));
        (
            self.cached_estimated_total(),
            self.total_cached_spilled_bytes.load(Relaxed),
        )
    }

    /// Debug builds: every clean document's cache equals a fresh computation.
    /// Skips documents that are busy, dirty, or marked while being checked.
    #[cfg(debug_assertions)]
    fn debug_assert_clean_caches_fresh(&self, active_id: Option<u64>) {
        for doc in self.snapshot_live_docs() {
            if Some(doc.pool_doc_id) == active_id || doc.stats_dirty.is_dirty() {
                continue;
            }
            // Compare while the statistics locks are held. A writer and another
            // refresh can otherwise replace the cache after the fresh read.
            let _ = fresh_doc_stats_unmarked(&doc);
        }
    }

    /// Upgrades every registered document without touching `LoroDocInner::user_handles`,
    /// so temporary registry references never make a document's last user handle skip
    /// its final commit.
    fn snapshot_live_docs(&self) -> Vec<RegistryRef> {
        let entries: Vec<_> = {
            let mut docs = self.docs.lock();
            docs.retain(|_, weak| weak.strong_count() > 0);
            docs.values().cloned().collect()
        };
        entries
            .into_iter()
            .filter_map(|weak| RegistryRef::upgrade(&weak))
            .collect()
    }

    fn update_cached_doc_bytes(
        &self,
        doc: &LoroDocInner,
        new_bytes: usize,
        new_spilled: Option<usize>,
    ) {
        swap_cached(
            &doc.cached_doc_bytes,
            &self.total_cached_doc_bytes,
            new_bytes,
        );
        if let Some(spilled) = new_spilled {
            swap_cached(
                &doc.cached_spilled_bytes,
                &self.total_cached_spilled_bytes,
                spilled,
            );
        }
    }

    fn current_heap_or_estimated_used(&self) -> u64 {
        if heap_tracker_active() {
            tracked_allocated_heap_bytes()
        } else {
            self.estimated_docs_total(None).0
        }
    }

    fn cached_estimated_total(&self) -> u64 {
        self.total_cached_doc_bytes
            .load(Relaxed)
            .saturating_add(self.global_block_cache.used_bytes() as u64)
    }

    fn reclaim_idle_docs_to_target(
        &self,
        live: &[RegistryRef],
        exclude_doc_id: Option<u64>,
        target_bytes: u64,
        force_full: bool,
    ) -> u64 {
        let mut cur_used = if heap_tracker_active() {
            tracked_allocated_heap_bytes()
        } else {
            self.cached_estimated_total()
        };
        if !force_full && cur_used <= target_bytes {
            return cur_used;
        }
        if self.global_block_cache.used_bytes() > 0 {
            self.global_block_cache.clear();
            cur_used = if heap_tracker_active() {
                tracked_allocated_heap_bytes()
            } else {
                self.cached_estimated_total()
            };
            if !force_full && cur_used <= target_bytes {
                return cur_used;
            }
        }

        let mut candidates: Vec<&RegistryRef> = live
            .iter()
            .filter(|doc| exclude_doc_id != Some(doc.pool_doc_id))
            .collect();
        candidates.sort_by_key(|doc| doc.last_access_tick.load(Relaxed));

        let mut stage1_done = FxHashSet::default();
        // Pass 1 (Stage 1): clear auxiliary caches, compact history, flush KV, shrink spare arena.
        for doc in &candidates {
            if !force_full && cur_used <= target_bytes {
                break;
            }
            if try_reclaim_idle_doc(doc, false, 0, false) {
                stage1_done.insert(doc.pool_doc_id);
            }
            cur_used = if heap_tracker_active() {
                tracked_allocated_heap_bytes()
            } else {
                self.cached_estimated_total()
            };
        }

        if !force_full && cur_used <= target_bytes {
            return cur_used;
        }

        // Pass 2 (Stage 2): demote cold materialized containers to compressed KV stores (LRU first).
        for doc in &candidates {
            if !force_full && cur_used <= target_bytes {
                break;
            }
            let need = if force_full {
                usize::MAX
            } else {
                usize::try_from(cur_used.saturating_sub(target_bytes)).unwrap_or(usize::MAX)
            };
            let _ = try_reclaim_idle_doc(doc, true, need, stage1_done.contains(&doc.pool_doc_id));
            cur_used = if heap_tracker_active() {
                tracked_allocated_heap_bytes()
            } else {
                self.cached_estimated_total()
            };
        }

        cur_used
    }

    fn reclaim_for_watermark(
        &self,
        target_bytes: u64,
        exclude_doc_id: Option<u64>,
        force_full: bool,
    ) {
        if !heap_tracker_active() {
            let _ = self.estimated_docs_total(None);
        }
        let live = self.snapshot_live_docs();
        let _ = self.reclaim_idle_docs_to_target(&live, exclude_doc_id, target_bytes, force_full);
    }

    fn enforce_global_watermarks_for_active(
        &self,
        active_doc: &LoroDoc,
        oplog: &OpLog,
        state: &mut DocState,
        uncommitted_txn_bytes: usize,
        active_bytes: usize,
    ) {
        self.update_cached_doc_bytes(active_doc, active_bytes, None);
        let budget = self.budget.lock().clone();
        let Some(high) = budget.effective_high_watermark() else {
            return;
        };
        let target = budget.effective_target_watermark().unwrap_or(high);
        let initial_used = if heap_tracker_active() {
            tracked_allocated_heap_bytes()
        } else {
            self.estimated_docs_total(Some((active_doc, active_bytes)))
                .0
        };
        let wasm_pressure = cfg!(target_arch = "wasm32")
            && budget.wasm_heap_pressure(wasm_linear_memory_bytes(), initial_used);
        if initial_used <= high && !wasm_pressure {
            self.record_soft_sweep_result(true, initial_used);
            return;
        }

        let effective_target = if wasm_pressure && initial_used <= high {
            target.min(budget.max_wasm_linear_bytes.unwrap().saturating_mul(65) / 100)
        } else {
            target
        };
        if initial_used <= effective_target && !wasm_pressure {
            return;
        }
        if self.soft_reclaim_backed_off(initial_used) {
            return;
        }

        let live = self.snapshot_live_docs();
        let mut cur_used = self.reclaim_idle_docs_to_target(
            &live,
            Some(active_doc.pool_doc_id),
            effective_target,
            false,
        );
        if cur_used <= effective_target {
            self.record_soft_sweep_result(true, cur_used);
            return;
        }

        // Fallback: if idle docs alone couldn't meet target, trim Stage 1 + Stage 2 on active doc.
        #[cfg(test)]
        ACTIVE_FALLBACK_PASSES.with(|n| n.set(n.get() + 1));
        state.evict_value_cache_to_bytes(0);
        state.reclaim_auxiliary_caches();
        oplog.compact_history();
        oplog.flush_kv();
        oplog.with_history_cache(|h| h.free_all());
        oplog.arena.shrink_spare_capacity();
        active_doc.diff_calculator.lock().reclaim_cold_caches(0);
        let _ = shrink_undo_managers_inner(active_doc);

        let _ = active_doc.memory_stats_locked(oplog, state, uncommitted_txn_bytes);
        cur_used = if heap_tracker_active() {
            tracked_allocated_heap_bytes()
        } else {
            self.cached_estimated_total()
        };
        if cur_used <= effective_target {
            self.record_soft_sweep_result(true, cur_used);
            return;
        }

        let need = usize::try_from(cur_used.saturating_sub(effective_target)).unwrap_or(usize::MAX);
        let target_state_bytes = state
            .state_estimated_bytes()
            .saturating_sub(need.saturating_mul(3) / 2);
        state.demote_cold_states(target_state_bytes, &Default::default());
        state.flush_kv();
        oplog.flush_kv();
        let _ = active_doc.memory_stats_locked(oplog, state, uncommitted_txn_bytes);
        cur_used = if heap_tracker_active() {
            tracked_allocated_heap_bytes()
        } else {
            self.cached_estimated_total()
        };
        self.record_soft_sweep_result(cur_used <= effective_target, cur_used);
    }

    fn check_global_budget_with_active(
        &self,
        active_doc: &LoroDocInner,
        active_bytes: Option<usize>,
        linear_before: Option<u64>,
    ) -> LoroResult<()> {
        if global_budget_flags() & BUDGET_FLAG_MAX_DOC == 0 {
            return Ok(());
        }
        let budget = self.budget.lock().clone();
        let used = if heap_tracker_active() {
            tracked_allocated_heap_bytes()
        } else {
            self.estimated_docs_total(active_bytes.map(|b| (active_doc, b)))
                .0
        };
        if let Some(max_heap) = budget.max_heap_bytes {
            if used > max_heap {
                return Err(make_global_budget_err(used, max_heap));
            }
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(max_lin) = budget.max_wasm_linear_bytes {
            let lin_now = wasm_linear_memory_bytes();
            if lin_now > max_lin {
                let fits_existing_pages = linear_before.is_some_and(|before| {
                    lin_now <= before && heap_fits_existing_pages(max_lin, used)
                });
                if !fits_existing_pages {
                    return Err(make_global_budget_err(lin_now, max_lin));
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        let _ = linear_before;
        Ok(())
    }
}

impl LoroDocInner {
    /// On the inner document, so the registry can arm a scope without creating a
    /// temporary `LoroDoc` handle while it holds the transaction lock.
    pub(crate) fn arm_budget_scope_on_txn(&self, txn: &mut Transaction) {
        if txn.budget_base_scope().is_some() || txn.has_rollback_scope() {
            return;
        }
        txn.doc_budget_limit = self.memory_budget.lock().max_doc_bytes;
        let id = LoroDoc::next_rollback_scope_id();
        txn.begin_budget_rollback_scope(id);
        self.state.lock().begin_local_rollback(id);
    }
}

/// A temporary strong reference taken by the global registry without touching
/// `LoroDocInner::user_handles`. See [`GlobalMemoryPool::snapshot_live_docs`].
pub(crate) struct RegistryRef {
    doc: Arc<LoroDocInner>,
}

impl RegistryRef {
    #[cfg(test)]
    fn from_doc(doc: &LoroDoc) -> Self {
        Self {
            doc: Arc::clone(&doc.inner),
        }
    }

    fn upgrade(weak: &std::sync::Weak<LoroDocInner>) -> Option<Self> {
        weak.upgrade().map(|doc| Self { doc })
    }
}

impl std::ops::Deref for RegistryRef {
    type Target = Arc<LoroDocInner>;

    fn deref(&self) -> &Self::Target {
        &self.doc
    }
}

fn global_pool() -> &'static GlobalMemoryPool {
    GLOBAL_POOL.get_or_init(GlobalMemoryPool::new)
}

pub(crate) fn unregister_doc_from_global_pool(
    pool_doc_id: u64,
    last_cached_bytes: usize,
    last_spilled_bytes: usize,
) {
    if pool_doc_id == 0 {
        return;
    }
    if let Some(pool) = GLOBAL_POOL.get() {
        pool.unregister(pool_doc_id, last_cached_bytes, last_spilled_bytes);
    }
}

fn shrink_undo_managers_inner(doc: &LoroDocInner) -> usize {
    let managers = {
        let mut guard = doc.undo_managers.lock();
        guard.retain(|w| w.strong_count() > 0);
        guard.iter().filter_map(|w| w.upgrade()).collect::<Vec<_>>()
    };
    let mut freed = 0usize;
    for manager in managers {
        if let Some(lock) = manager.try_lock() {
            if let Ok(mut inner) = lock.try_borrow_mut() {
                let before = inner.used_bytes();
                inner.shrink_to_fit();
                let after = inner.used_bytes();
                freed = freed.saturating_add(before.saturating_sub(after));
            }
        }
    }
    freed
}

/// The undo managers' bytes, and whether every manager could be read.
fn sum_undo_bytes_inner(doc: &LoroDocInner) -> (usize, bool) {
    let managers = {
        let mut guard = doc.undo_managers.lock();
        guard.retain(|w| w.strong_count() > 0);
        guard.iter().filter_map(|w| w.upgrade()).collect::<Vec<_>>()
    };
    let mut total = 0usize;
    let mut complete = true;
    for manager in managers {
        match manager.try_lock() {
            Some(lock) => match lock.try_borrow() {
                Ok(inner) => total = total.saturating_add(inner.used_bytes()),
                Err(_) => complete = false,
            },
            None => complete = false,
        }
    }
    (total, complete)
}

fn compute_doc_stats_locked_inner(
    doc: &LoroDocInner,
    oplog: &OpLog,
    state: &DocState,
    uncommitted_txn_bytes: usize,
    try_diff_only: bool,
) -> DocMemoryStats {
    let diff_calc_bytes = if try_diff_only {
        doc.diff_calculator
            .try_lock()
            .map_or(0, |d| d.estimated_bytes())
    } else {
        doc.diff_calculator.lock().estimated_bytes()
    };
    let undo_bytes = sum_undo_bytes_inner(doc).0;
    let stats = doc_stats_from_parts(
        doc,
        oplog,
        state,
        uncommitted_txn_bytes,
        diff_calc_bytes,
        undo_bytes,
    );
    if doc.pool_doc_id != 0 {
        if let Some(pool) = GLOBAL_POOL.get() {
            pool.update_cached_doc_bytes(doc, stats.total_bytes, Some(stats.spilled_disk_bytes));
        } else {
            doc.cached_doc_bytes.store(stats.total_bytes, Release);
            doc.cached_spilled_bytes
                .store(stats.spilled_disk_bytes, Release);
        }
    }
    stats
}

/// The statistics from their parts; no side effects on the caches.
fn doc_stats_from_parts(
    doc: &LoroDocInner,
    oplog: &OpLog,
    state: &DocState,
    uncommitted_txn_bytes: usize,
    diff_calc_bytes: usize,
    undo_bytes: usize,
) -> DocMemoryStats {
    let oplog_mem = oplog.estimated_memory_breakdown();
    let state_bytes = state.state_estimated_bytes();
    let value_cache_bytes = state.value_cache_bytes();
    let state_kv_bytes = state.kv_estimated_bytes();
    let state_spilled = state.spilled_disk_bytes();
    let kv_bytes = state_kv_bytes.saturating_add(oplog_mem.change_kv_bytes);
    let block_cache_bytes = doc.kv_block_cache.local_used_bytes();
    let spilled_disk_bytes = state_spilled.saturating_add(oplog_mem.spilled_disk_bytes);

    let total_bytes = state_bytes
        .saturating_add(value_cache_bytes)
        .saturating_add(kv_bytes)
        .saturating_add(block_cache_bytes)
        .saturating_add(oplog_mem.change_cache_bytes)
        .saturating_add(oplog_mem.dag_bytes)
        .saturating_add(oplog_mem.arena_bytes)
        .saturating_add(oplog_mem.pending_bytes)
        .saturating_add(oplog_mem.history_cache_bytes)
        .saturating_add(diff_calc_bytes)
        .saturating_add(undo_bytes)
        .saturating_add(uncommitted_txn_bytes);

    DocMemoryStats {
        total_bytes,
        state_bytes,
        value_cache_bytes,
        kv_bytes,
        block_cache_bytes,
        change_cache_bytes: oplog_mem.change_cache_bytes,
        dag_bytes: oplog_mem.dag_bytes,
        arena_bytes: oplog_mem.arena_bytes,
        pending_bytes: oplog_mem.pending_bytes,
        history_cache_bytes: oplog_mem.history_cache_bytes,
        diff_calc_bytes,
        undo_bytes,
        uncommitted_txn_bytes,
        spilled_disk_bytes,
    }
}

/// Recomputes `doc`'s cached statistics if none of its locks (nor its undo
/// managers) are busy, and clears its dirty flag. Returns `false`, leaving the
/// flag set, when it could not; the caller keeps the document queued. Takes the
/// locks without marking: this is the reader the marks are for (see
/// [`DocStatsDirty`]).
fn refresh_cached_doc_bytes_for_inner(doc: &LoroDocInner) -> bool {
    let Some(txn) = doc.txn.try_lock_unmarked() else {
        return false;
    };
    let Some(oplog) = doc.oplog.try_lock_unmarked() else {
        return false;
    };
    let Some(state) = doc.state.try_lock_unmarked() else {
        return false;
    };
    let Some(diff_calc) = doc.diff_calculator.try_lock_unmarked() else {
        return false;
    };
    // Clear before reading: a change made from now on marks again.
    doc.stats_dirty
        .dirty
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let uncommitted = txn
        .as_ref()
        .map_or(0, Transaction::estimated_uncommitted_bytes);
    let (undo_bytes, undo_complete) = sum_undo_bytes_inner(doc);
    let stats = doc_stats_from_parts(
        doc,
        &oplog,
        &state,
        uncommitted,
        diff_calc.estimated_bytes(),
        undo_bytes,
    );
    if doc.pool_doc_id != 0 {
        if let Some(pool) = GLOBAL_POOL.get() {
            pool.update_cached_doc_bytes(doc, stats.total_bytes, Some(stats.spilled_disk_bytes));
        }
    }
    if !undo_complete {
        doc.stats_dirty
            .dirty
            .store(true, std::sync::atomic::Ordering::SeqCst);
        return false;
    }
    true
}

/// `doc`'s statistics, computed without marking it or updating its cache, or
/// `None` if something is busy.
#[cfg(debug_assertions)]
fn fresh_doc_stats_unmarked(doc: &LoroDocInner) -> Option<DocMemoryStats> {
    let txn = doc.txn.try_lock_unmarked()?;
    let oplog = doc.oplog.try_lock_unmarked()?;
    let state = doc.state.try_lock_unmarked()?;
    let diff_calc = doc.diff_calculator.try_lock_unmarked()?;
    let (undo_bytes, complete) = sum_undo_bytes_inner(doc);
    if !complete {
        return None;
    }
    let uncommitted = txn
        .as_ref()
        .map_or(0, Transaction::estimated_uncommitted_bytes);
    let stats = doc_stats_from_parts(
        doc,
        &oplog,
        &state,
        uncommitted,
        diff_calc.estimated_bytes(),
        undo_bytes,
    );
    if !doc.stats_dirty.is_dirty() {
        let cached = doc
            .cached_doc_bytes
            .load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            cached, stats.total_bytes,
            "doc {} is not marked dirty but its cached statistics are stale: {stats:?}",
            doc.pool_doc_id,
        );
    }
    Some(stats)
}

fn swap_cached(
    cached: &std::sync::atomic::AtomicUsize,
    total: &std::sync::atomic::AtomicU64,
    new: usize,
) {
    let prev = cached.swap(new, AcqRel);
    if new > prev {
        total.fetch_add((new - prev) as u64, Relaxed);
    } else if new < prev {
        sub_saturating(total, prev - new);
    }
}

fn sub_saturating(total: &std::sync::atomic::AtomicU64, diff: usize) {
    if diff > 0 {
        let diff = diff as u64;
        let _ = total.fetch_update(Relaxed, Relaxed, |cur| Some(cur.saturating_sub(diff)));
    }
}

#[cfg(test)]
thread_local! {
    static IDLE_STAGE1_PASSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static ACTIVE_FALLBACK_PASSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static DIRTY_REFRESHES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static DOC_STAGE1_PASSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn try_reclaim_idle_doc(
    doc: &Arc<LoroDocInner>,
    stage2: bool,
    target_reduction_bytes: usize,
    stage1_completed: bool,
) -> bool {
    let Some(txn_guard) = doc.txn.try_lock() else {
        return false;
    };
    if txn_guard
        .as_ref()
        .is_some_and(|txn| !txn.is_empty() || txn.has_rollback_scope())
    {
        return false;
    }
    let Some(oplog) = doc.oplog.try_lock() else {
        return false;
    };
    if oplog.batch_importing || oplog.has_import_rollback() || oplog.dag.has_pending_txn() {
        return false;
    }
    let Some(mut state) = doc.state.try_lock() else {
        return false;
    };

    if !stage1_completed {
        #[cfg(test)]
        IDLE_STAGE1_PASSES.with(|n| n.set(n.get() + 1));
        state.evict_value_cache_to_bytes(0);
        if !doc.kv_block_cache.is_delegating_to_parent() {
            doc.kv_block_cache.clear();
        }
        state.reclaim_auxiliary_caches();
        oplog.compact_history();
        oplog.flush_kv();
        oplog.with_history_cache(|h| h.free_all());
        oplog.arena.shrink_spare_capacity();
        if let Some(mut diff_calc) = doc.diff_calculator.try_lock() {
            diff_calc.reclaim_cold_caches(0);
        }
        let _ = shrink_undo_managers_inner(doc);
    }

    if stage2 {
        let cur_state_bytes = state.state_estimated_bytes();
        let target_state_bytes = if target_reduction_bytes == usize::MAX {
            0
        } else {
            let demote_budget = target_reduction_bytes.saturating_mul(3) / 2;
            cur_state_bytes.saturating_sub(demote_budget)
        };
        state.demote_cold_states(target_state_bytes, &Default::default());
        state.flush_kv();
        oplog.flush_kv();
    }

    let _ = compute_doc_stats_locked_inner(doc, &oplog, &state, 0, true);
    true
}

/// Configures process-wide (or WASM-instance-wide) memory watermarks, shared KV block
/// cache size, and hard heap / linear-memory limits across all live [`LoroDoc`] instances.
pub fn set_global_memory_budget(budget: GlobalMemoryBudget) {
    let pool = global_pool();
    let enabled = budget.is_enabled();
    let flags = compute_global_flags(&budget);
    pool.global_block_cache
        .set_max_bytes(budget.effective_block_cache_bytes());
    *pool.budget.lock() = budget;
    pool.record_soft_sweep_result(true, 0);
    GLOBAL_BUDGET_FLAGS.store(flags, Release);

    let live = pool.snapshot_live_docs();
    if enabled {
        let arm_hard_scope = flags & BUDGET_FLAG_MAX_DOC != 0;
        for doc in &live {
            doc.kv_block_cache
                .set_parent(Some(&pool.global_block_cache));
            // Marks made while no budget was set did not queue the document.
            if !refresh_cached_doc_bytes_for_inner(doc) {
                pool.push_dirty(doc.pool_doc_id);
            }
            if arm_hard_scope {
                if let Some(mut txn_guard) = doc.txn.try_lock() {
                    if let Some(txn) = txn_guard.as_mut() {
                        if txn.is_empty()
                            && !txn.has_rollback_scope()
                            && txn.budget_base_scope().is_none()
                        {
                            doc.arm_budget_scope_on_txn(txn);
                        }
                    }
                }
            }
        }
        if flags & BUDGET_FLAG_WATERMARKS != 0 {
            let high = pool.budget.lock().effective_high_watermark();
            let target = pool.budget.lock().effective_target_watermark();
            if let Some(h) = high {
                let used = pool.current_heap_or_estimated_used();
                if used > h {
                    pool.reclaim_for_watermark(target.unwrap_or(h), None, false);
                }
            }
        }
    } else {
        for doc in &live {
            doc.kv_block_cache.set_parent(None);
        }
        pool.global_block_cache.clear();
    }
}

/// Returns a snapshot of the active [`GlobalMemoryBudget`] configuration.
pub fn global_memory_budget() -> GlobalMemoryBudget {
    global_pool().budget.lock().clone()
}

/// Returns global memory statistics across all live [`LoroDoc`] instances and the active
/// global allocator (if tracked, as on WASM).
pub fn global_memory_stats() -> GlobalMemoryStats {
    let pool = global_pool();
    let live_doc_count = pool.live_doc_count();
    let (docs_estimated_bytes, spilled_disk_bytes) = pool.estimated_docs_total(None);
    let global_block_cache_bytes = pool.global_block_cache.used_bytes();
    GlobalMemoryStats {
        live_doc_count,
        docs_estimated_bytes,
        global_block_cache_bytes,
        allocated_heap_bytes: tracked_allocated_heap_bytes(),
        peak_allocated_heap_bytes: tracked_peak_allocated_heap_bytes(),
        wasm_linear_memory_bytes: wasm_linear_memory_bytes(),
        spilled_disk_bytes,
    }
}

/// Runs cross-document reclamation across idle [`LoroDoc`]s down to `target_bytes`
/// (or full Stage 1 + Stage 2 reclamation across all idle docs when `None`),
/// returning the observed byte reduction.
pub fn reclaim_global_memory(target_bytes: Option<u64>) -> u64 {
    let pool = global_pool();
    let before = if heap_tracker_active() {
        tracked_allocated_heap_bytes()
    } else {
        pool.estimated_docs_total(None).0
    };
    let live = pool.snapshot_live_docs();
    let (target, force_full) = match target_bytes {
        Some(t) => (t, false),
        None => (0, true),
    };
    let after = pool.reclaim_idle_docs_to_target(&live, None, target, force_full);
    before.saturating_sub(after)
}

/// Returns `true` when running on `wasm32` with an active global WASM linear-memory ceiling.
#[inline]
pub fn has_global_wasm_linear_limit() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        if global_budget_flags() & BUDGET_FLAG_MAX_DOC == 0 {
            return false;
        }
        global_pool().budget.lock().max_wasm_linear_bytes.is_some()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

/// Checks whether allocating `needed_bytes` on the WASM heap would exceed
/// `GlobalMemoryBudget::max_wasm_linear_bytes`, reclaiming idle docs first when possible.
pub fn preflight_wasm_linear_alloc(needed_bytes: u64) -> LoroResult<()> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = needed_bytes;
        Ok(())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let pool = global_pool();
        let Some(max_lin) = pool.budget.lock().max_wasm_linear_bytes else {
            return Ok(());
        };
        let mut used = tracked_allocated_heap_bytes();
        if used.saturating_add(needed_bytes) > max_lin.saturating_mul(80) / 100 {
            pool.reclaim_for_watermark(max_lin.saturating_mul(50) / 100, None, false);
            used = tracked_allocated_heap_bytes();
        }
        let projected = used.saturating_add(needed_bytes);
        if projected > max_lin {
            return Err(make_global_budget_err(projected, max_lin));
        }
        Ok(())
    }
}

/// Runs full global reclamation and checks whether global usage now satisfies
/// [`GlobalMemoryBudget::max_heap_bytes`] and [`GlobalMemoryBudget::max_wasm_linear_bytes`].
pub fn try_reclaim_global_to_budget() -> LoroResult<GlobalMemoryStats> {
    let _ = reclaim_global_memory(None);
    let stats = global_memory_stats();
    let budget = global_memory_budget();
    if let Some(max_heap) = budget.max_heap_bytes {
        let used = if heap_tracker_active() {
            stats.allocated_heap_bytes
        } else {
            stats.docs_estimated_bytes
        };
        if used > max_heap {
            return Err(make_global_budget_err(used, max_heap));
        }
    }
    #[cfg(target_arch = "wasm32")]
    if let Some(max_lin) = budget.max_wasm_linear_bytes {
        if !linear_budget_satisfied(
            stats.wasm_linear_memory_bytes,
            max_lin,
            stats.allocated_heap_bytes,
        ) {
            return Err(make_global_budget_err(
                stats.wasm_linear_memory_bytes,
                max_lin,
            ));
        }
    }
    Ok(stats)
}

/// Live heap that counts as within `max_wasm_linear_bytes` once the linear
/// memory itself has grown past it: WASM pages never shrink, so only heap use
/// can come back down. Shared by hard admission (which also requires that the
/// commit added no pages) and [`try_reclaim_global_to_budget`].
fn heap_fits_existing_pages(max_lin: u64, used_heap: u64) -> bool {
    used_heap <= max_lin.saturating_mul(70) / 100
}

/// Whether `max_wasm_linear_bytes` is met after a reclamation: the pages are
/// within it, or they grew past it (permanently) but the live heap is back under
/// the share [`heap_fits_existing_pages`] allows, the same rule `try_commit`
/// uses. Before, `try_reclaim_global_to_budget` failed forever once the pages
/// had grown, while commits still succeeded. See context/memory-budget.md,
/// "WASM linear memory ceiling".
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn linear_budget_satisfied(lin_now: u64, max_lin: u64, used_heap: u64) -> bool {
    lin_now <= max_lin || heap_fits_existing_pages(max_lin, used_heap)
}

/// Whether the WASM linear memory has grown past
/// [`GlobalMemoryBudget::max_wasm_linear_bytes`]. It cannot shrink again; an
/// application may want to recreate the instance once its documents are saved.
/// Always `false` on native targets.
pub fn wasm_linear_memory_exceeded() -> bool {
    let lin = wasm_linear_memory_bytes();
    global_pool()
        .budget
        .lock()
        .max_wasm_linear_bytes
        .is_some_and(|max_lin| lin > max_lin)
}

impl LoroDoc {
    /// Configures soft memory watermarks and hard memory quotas for this document.
    /// A new hard document quota applies to the next transaction. Pending edits
    /// retain the quota captured when that transaction began accepting edits.
    ///
    /// If moving the document to a new `spill_dir` fails, the document keeps its
    /// current storage (and [`Self::memory_budget`] keeps reporting the previous
    /// directory, so setting the same directory again retries), the failure is
    /// logged, and the other settings are applied. Use
    /// [`Self::try_set_memory_budget`] to get the error instead.
    pub fn set_memory_budget(&self, budget: MemoryBudget) {
        let _ = self.set_memory_budget_inner(budget, false);
    }

    /// Like [`Self::set_memory_budget`], but if moving the document to a new
    /// `spill_dir` fails (the directory cannot be created, or the spilled tables
    /// cannot be written), returns the error and changes nothing.
    pub fn try_set_memory_budget(&self, budget: MemoryBudget) -> LoroResult<()> {
        self.set_memory_budget_inner(budget, true)
    }

    fn set_memory_budget_inner(&self, mut budget: MemoryBudget, strict: bool) -> LoroResult<()> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let prev_spill_dir = self.memory_budget.lock().spill_dir.clone();
            if prev_spill_dir != budget.spill_dir {
                if let Err(err) = self.switch_spill_backend(budget.spill_dir.as_deref()) {
                    let err = LoroError::Unknown(
                        format!("failed to switch the out-of-core spill directory: {err}")
                            .into_boxed_str(),
                    );
                    if strict {
                        return Err(err);
                    }
                    tracing::warn!(dir = ?budget.spill_dir, "{err}");
                    budget.spill_dir = prev_spill_dir;
                }
            }
        }
        let _ = strict;
        #[cfg(not(target_arch = "wasm32"))]
        if budget.spill_dir.is_some() && budget.effective_high_watermark().is_none() {
            // A spill directory only chooses where flushed tables live; nothing
            // flushes automatically without watermarks or `max_doc_bytes`.
            // See context/memory-budget.md, "Spill directory without a budget".
            tracing::warn!(
                dir = ?budget.spill_dir,
                "spill_dir is set without watermarks or max_doc_bytes: memory moves to disk \
                 only on reclaim_memory"
            );
        }
        self.soft_backoff.reset();
        let mut flags = 0u8;
        if budget.max_doc_bytes.is_some() {
            flags |= BUDGET_FLAG_MAX_DOC;
        }
        if budget.effective_high_watermark().is_some()
            || budget.max_kv_block_cache_bytes.is_some()
            || budget.max_decoded_value_cache_bytes.is_some()
            || budget.max_undo_bytes.is_some()
        {
            flags |= BUDGET_FLAG_WATERMARKS;
        }
        if budget.max_import_blob_bytes.is_some() {
            flags |= BUDGET_FLAG_MAX_IMPORT_BLOB;
        }
        self.kv_block_cache
            .set_explicit_max_bytes(budget.max_kv_block_cache_bytes);
        self.config.set_decoded_value_cache_bytes_limit(
            budget.max_decoded_value_cache_bytes.unwrap_or(usize::MAX),
        );
        let max_pending = budget.max_pending_bytes;
        let max_undo = budget.max_undo_bytes;
        let has_max_doc = budget.max_doc_bytes.is_some();
        *self.memory_budget.lock() = budget;
        self.budget_flags.store(flags, Release);

        self.oplog.lock().set_max_pending_bytes(max_pending);
        if let Some(max_undo_bytes) = max_undo {
            for manager in self.live_undo_managers() {
                if let Some(lock) = manager.try_lock() {
                    if let Ok(mut inner) = lock.try_borrow_mut() {
                        inner.set_max_undo_bytes_from_doc(max_undo_bytes);
                    };
                }
            }
        }

        if !self.txn.is_held_by_current_thread() {
            let mut txn_guard = self.txn.lock();
            if let Some(txn) = txn_guard.as_mut() {
                if txn.is_empty() && !txn.has_rollback_scope() {
                    if has_max_doc && txn.budget_base_scope().is_none() {
                        self.arm_budget_scope_on_txn(txn);
                    }
                    txn.doc_budget_limit = self.memory_budget.lock().max_doc_bytes;
                }
            }
        }

        self.enforce_memory_watermarks();
        Ok(())
    }

    /// Move the op log and state stores (including a shallow root) to new
    /// backends: out-of-core files under `dir`, or memory for `None`. Both new
    /// stores are filled and flushed before either is installed, under the op
    /// log and state locks, so a failure leaves the current stores in place.
    #[cfg(not(target_arch = "wasm32"))]
    fn switch_spill_backend(&self, dir: Option<&std::path::Path>) -> Result<(), String> {
        let block_cache = self.kv_block_cache.clone();
        let (oplog_kv, state_kv): (
            Box<dyn crate::kv_store::KvStore>,
            Arc<crate::sync::Mutex<dyn crate::kv_store::KvStore>>,
        ) = match dir {
            Some(dir) => {
                let make = |config: crate::kv_store::MemKvConfig| {
                    crate::kv_store::FileKvStore::new(dir, config)
                        .map_err(|e| format!("{}: {e}", dir.display()))
                };
                (
                    Box::new(make(
                        crate::kv_store::MemKvConfig::default().block_cache(block_cache.clone()),
                    )?),
                    Arc::new(crate::sync::Mutex::new(make(
                        crate::kv_store::MemKvConfig::default()
                            .should_encode_none(false)
                            .block_cache(block_cache.clone()),
                    )?)),
                )
            }
            None => (
                Box::new(crate::kv_store::MemKvStore::new(
                    crate::kv_store::MemKvConfig::default().block_cache(block_cache.clone()),
                )),
                Arc::new(crate::sync::Mutex::new(crate::kv_store::MemKvStore::new(
                    crate::kv_store::MemKvConfig::default()
                        .should_encode_none(false)
                        .block_cache(block_cache.clone()),
                ))),
            ),
        };
        let oplog = self.oplog.lock();
        let mut state = self.state.lock();
        let oplog_kv = oplog.prepare_kv_backend(oplog_kv)?;
        let state_kv = state.prepare_kv_backend(state_kv)?;
        oplog.install_kv_backend(oplog_kv);
        state.install_kv_backend(state_kv);
        Ok(())
    }

    /// Returns a clone of the current [`MemoryBudget`] configuration.
    pub fn memory_budget(&self) -> MemoryBudget {
        self.memory_budget.lock().clone()
    }

    #[inline]
    pub(crate) fn touch_access_tick(&self) {
        if global_budget_flags() != 0 {
            let tick = GLOBAL_ACCESS_TICK.fetch_add(1, Relaxed);
            self.last_access_tick.store(tick, Relaxed);
        }
    }

    pub(crate) fn on_commit_completed(&self) {
        self.touch_access_tick();
        if self.has_watermark_budget() {
            self.enforce_memory_watermarks();
        } else if global_budget_flags() != 0 {
            self.refresh_cached_doc_bytes_unlocked();
        }
    }

    pub(crate) fn refresh_cached_doc_bytes_unlocked(&self) {
        if self.pool_doc_id == 0 {
            return;
        }
        let uncommitted = if self.txn.is_held_by_current_thread() {
            0
        } else {
            self.txn
                .try_lock()
                .and_then(|g| g.as_ref().map(Transaction::estimated_uncommitted_bytes))
                .unwrap_or(0)
        };
        if let (Some(oplog), Some(state)) = (self.oplog.try_lock(), self.state.try_lock()) {
            let _ = compute_doc_stats_locked_inner(self, &oplog, &state, uncommitted, true);
        }
    }

    #[inline]
    pub(crate) fn has_max_doc_bytes(&self) -> bool {
        (self.budget_flags.load(Acquire) | global_budget_flags()) & BUDGET_FLAG_MAX_DOC != 0
    }

    #[inline]
    fn has_watermark_budget(&self) -> bool {
        (self.budget_flags.load(Acquire) | global_budget_flags()) & BUDGET_FLAG_WATERMARKS != 0
    }

    #[inline]
    fn has_max_import_blob_bytes(&self) -> bool {
        (self.budget_flags.load(Acquire) | global_budget_flags()) & BUDGET_FLAG_MAX_IMPORT_BLOB != 0
    }

    pub fn check_import_blob_limit(&self, len: usize) -> LoroResult<()> {
        self.preflight_import_bytes(len, false)
    }

    /// Pre-allocation check called by bindings (such as `loro-wasm`) immediately before copying
    /// an external `len`-byte buffer into WASM linear memory / Rust heap.
    pub fn preflight_external_import_alloc(&self, len: usize) -> LoroResult<()> {
        self.preflight_import_bytes(len, true)
    }

    fn preflight_import_bytes(&self, len: usize, include_buffer_alloc: bool) -> LoroResult<()> {
        if !self.has_max_import_blob_bytes() {
            return Ok(());
        }
        if self.budget_flags.load(Acquire) & BUDGET_FLAG_MAX_IMPORT_BLOB != 0 {
            if let Some(limit) = self.memory_budget.lock().max_import_blob_bytes {
                if len > limit {
                    return Err(LoroError::memory_limit_exceeded(
                        MemoryLimitKind::ImportBlob,
                        len,
                        limit,
                    ));
                }
            }
        }
        if global_budget_flags() & BUDGET_FLAG_MAX_IMPORT_BLOB != 0 {
            let pool = global_pool();
            let budget = pool.budget.lock().clone();
            if let Some(limit) = budget.max_import_blob_bytes {
                if len > limit {
                    return Err(LoroError::memory_limit_exceeded(
                        MemoryLimitKind::ImportBlob,
                        len,
                        limit,
                    ));
                }
            }
            let wasm_limit = if cfg!(target_arch = "wasm32") {
                budget.max_wasm_linear_bytes
            } else {
                None
            };
            let hard_cap = match (budget.max_heap_bytes, wasm_limit) {
                (Some(h), Some(w)) => Some(h.min(w)),
                (Some(h), None) => Some(h),
                (None, Some(w)) => Some(w),
                (None, None) => None,
            };
            if let Some(cap) = hard_cap {
                let len_u64 = len as u64;
                if len_u64 > cap {
                    return Err(make_global_budget_err(len_u64, cap));
                }
                let mut used = pool.current_heap_or_estimated_used();
                let high = budget.effective_high_watermark().unwrap_or(cap);
                let target = budget.effective_target_watermark().unwrap_or(high);
                let head_target = target.min(cap.saturating_sub(len_u64));
                if used.saturating_add(len_u64) > high.min(cap) {
                    let exclude = if self.txn.is_held_by_current_thread()
                        || self.oplog.is_locked()
                        || self.state.is_locked()
                    {
                        Some(self.pool_doc_id)
                    } else {
                        None
                    };
                    pool.reclaim_for_watermark(head_target, exclude, false);
                    used = pool.current_heap_or_estimated_used();
                }
                if include_buffer_alloc {
                    let projected = used.saturating_add(len_u64);
                    if let Some(max_heap) = budget.max_heap_bytes {
                        if projected > max_heap {
                            return Err(make_global_budget_err(projected, max_heap));
                        }
                    }
                    if let Some(max_lin) = wasm_limit {
                        if projected > max_lin {
                            return Err(make_global_budget_err(projected, max_lin));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn check_import_json_limit(&self, json: &JsonSchema) -> LoroResult<()> {
        if !self.has_max_import_blob_bytes() {
            return Ok(());
        }
        struct CountingWriter {
            count: usize,
            limit: usize,
        }
        impl std::io::Write for CountingWriter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.count = self.count.saturating_add(buf.len());
                if self.count > self.limit {
                    return Err(std::io::Error::other("import json exceeds limit"));
                }
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        let local_limit = if self.budget_flags.load(Acquire) & BUDGET_FLAG_MAX_IMPORT_BLOB != 0 {
            self.memory_budget.lock().max_import_blob_bytes
        } else {
            None
        };
        let global_limit = if global_budget_flags() & BUDGET_FLAG_MAX_IMPORT_BLOB != 0 {
            global_pool().budget.lock().max_import_blob_bytes
        } else {
            None
        };
        let effective_limit = match (local_limit, global_limit) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let Some(limit) = effective_limit else {
            return Ok(());
        };
        let mut writer = CountingWriter { count: 0, limit };
        if serde_json::to_writer(&mut writer, json).is_err() || writer.count > limit {
            return Err(LoroError::memory_limit_exceeded(
                MemoryLimitKind::ImportBlob,
                writer.count.max(limit.saturating_add(1)),
                limit,
            ));
        }
        Ok(())
    }

    fn live_undo_managers(
        &self,
    ) -> Vec<Arc<parking_lot::ReentrantMutex<std::cell::RefCell<crate::undo::UndoManagerInner>>>>
    {
        let mut guard = self.undo_managers.lock();
        guard.retain(|w| w.strong_count() > 0);
        guard.iter().filter_map(|w| w.upgrade()).collect()
    }

    fn shrink_undo_managers(&self) -> usize {
        shrink_undo_managers_inner(self)
    }

    /// Returns a snapshot of estimated RAM and disk usage across all document subsystems.
    pub fn memory_stats(&self) -> DocMemoryStats {
        let uncommitted_txn_bytes = if self.txn.is_held_by_current_thread() {
            0
        } else {
            self.txn
                .lock()
                .as_ref()
                .map_or(0, Transaction::estimated_uncommitted_bytes)
        };
        self.memory_stats_with_txn_bytes(uncommitted_txn_bytes)
    }

    fn memory_stats_with_txn_bytes(&self, uncommitted_txn_bytes: usize) -> DocMemoryStats {
        let oplog = self.oplog.lock();
        let state = self.state.lock();
        self.memory_stats_locked(&oplog, &state, uncommitted_txn_bytes)
    }

    fn memory_stats_locked(
        &self,
        oplog: &OpLog,
        state: &DocState,
        uncommitted_txn_bytes: usize,
    ) -> DocMemoryStats {
        compute_doc_stats_locked_inner(self, oplog, state, uncommitted_txn_bytes, false)
    }

    pub(crate) fn check_doc_budget_with_limit(
        &self,
        uncommitted_txn_bytes: usize,
        limit: usize,
    ) -> LoroResult<()> {
        let stats = self.memory_stats_with_txn_bytes(uncommitted_txn_bytes);
        if stats.total_bytes > limit {
            return Err(LoroError::memory_limit_exceeded(
                MemoryLimitKind::DocBudget,
                stats.total_bytes,
                limit,
            ));
        }
        Ok(())
    }

    pub(crate) fn check_transaction_budget(
        &self,
        txn: &Transaction,
        linear_before: Option<u64>,
    ) -> LoroResult<()> {
        if let Some(limit) = txn.doc_budget_limit {
            self.check_doc_budget_with_limit(txn.estimated_uncommitted_bytes(), limit)?;
        }
        if global_budget_flags() & BUDGET_FLAG_MAX_DOC != 0 && txn.budget_base_scope().is_some() {
            let active_bytes = if heap_tracker_active() {
                None
            } else {
                Some(
                    self.memory_stats_with_txn_bytes(txn.estimated_uncommitted_bytes())
                        .total_bytes,
                )
            };
            global_pool().check_global_budget_with_active(self, active_bytes, linear_before)?;
        }
        Ok(())
    }

    fn check_doc_budget_with_txn_bytes_and_linear(
        &self,
        uncommitted_txn_bytes: usize,
        linear_before: Option<u64>,
    ) -> LoroResult<()> {
        let local_limit = if self.budget_flags.load(Acquire) & BUDGET_FLAG_MAX_DOC != 0 {
            self.memory_budget.lock().max_doc_bytes
        } else {
            None
        };
        let has_global = global_budget_flags() & BUDGET_FLAG_MAX_DOC != 0;
        if local_limit.is_none() && !has_global {
            return Ok(());
        }
        let need_stats = local_limit.is_some() || (has_global && !heap_tracker_active());
        let active_bytes = if need_stats {
            let stats = self.memory_stats_with_txn_bytes(uncommitted_txn_bytes);
            if let Some(limit) = local_limit {
                if stats.total_bytes > limit {
                    return Err(LoroError::memory_limit_exceeded(
                        MemoryLimitKind::DocBudget,
                        stats.total_bytes,
                        limit,
                    ));
                }
            }
            Some(stats.total_bytes)
        } else {
            None
        };
        if has_global {
            global_pool().check_global_budget_with_active(self, active_bytes, linear_before)?;
        }
        Ok(())
    }

    fn check_doc_budget_locked_with_linear(
        &self,
        oplog: &OpLog,
        state: &DocState,
        uncommitted_txn_bytes: usize,
        linear_before: Option<u64>,
    ) -> LoroResult<()> {
        let local_limit = if self.budget_flags.load(Acquire) & BUDGET_FLAG_MAX_DOC != 0 {
            self.memory_budget.lock().max_doc_bytes
        } else {
            None
        };
        let has_global = global_budget_flags() & BUDGET_FLAG_MAX_DOC != 0;
        if local_limit.is_none() && !has_global {
            return Ok(());
        }
        let need_stats = local_limit.is_some() || (has_global && !heap_tracker_active());
        let active_bytes = if need_stats {
            let stats = self.memory_stats_locked(oplog, state, uncommitted_txn_bytes);
            if let Some(limit) = local_limit {
                if stats.total_bytes > limit {
                    return Err(LoroError::memory_limit_exceeded(
                        MemoryLimitKind::DocBudget,
                        stats.total_bytes,
                        limit,
                    ));
                }
            }
            Some(stats.total_bytes)
        } else {
            None
        };
        if has_global {
            global_pool().check_global_budget_with_active(self, active_bytes, linear_before)?;
        }
        Ok(())
    }

    pub(crate) fn enforce_memory_watermarks(&self) {
        if !self.has_watermark_budget() {
            return;
        }
        let uncommitted_txn_bytes = if self.txn.is_held_by_current_thread() {
            0
        } else {
            self.txn
                .lock()
                .as_ref()
                .map_or(0, Transaction::estimated_uncommitted_bytes)
        };
        self.enforce_memory_watermarks_with_txn_bytes(uncommitted_txn_bytes, false);
    }

    pub(crate) fn enforce_memory_watermarks_with_txn_bytes(
        &self,
        uncommitted_txn_bytes: usize,
        force_full: bool,
    ) {
        if !force_full && !self.has_watermark_budget() {
            return;
        }
        let budget = self.memory_budget.lock().clone();
        if !force_full
            && heap_tracker_active()
            && budget == MemoryBudget::default()
            && global_watermarks_active()
        {
            let global = global_pool().budget.lock().clone();
            let used = tracked_allocated_heap_bytes();
            if global
                .effective_high_watermark()
                .is_some_and(|high| used <= high)
                && !(cfg!(target_arch = "wasm32")
                    && global.wasm_heap_pressure(wasm_linear_memory_bytes(), used))
            {
                return;
            }
        }
        if !force_full
            && !global_watermarks_active()
            && budget.effective_high_watermark().is_none()
            && budget.max_kv_block_cache_bytes.is_none()
            && budget.max_decoded_value_cache_bytes.is_none()
            && budget.max_undo_bytes.is_none()
        {
            return;
        }
        let oplog = self.oplog.lock();
        let mut state = self.state.lock();
        self.enforce_watermarks_with_budget_locked(
            &budget,
            &oplog,
            &mut state,
            uncommitted_txn_bytes,
            force_full,
        );
    }

    fn enforce_watermarks_locked(
        &self,
        oplog: &OpLog,
        state: &mut DocState,
        uncommitted_txn_bytes: usize,
        force_full: bool,
    ) {
        if !force_full && !self.has_watermark_budget() {
            return;
        }
        let budget = self.memory_budget.lock().clone();
        if !force_full
            && !global_watermarks_active()
            && budget.effective_high_watermark().is_none()
            && budget.max_kv_block_cache_bytes.is_none()
            && budget.max_decoded_value_cache_bytes.is_none()
            && budget.max_undo_bytes.is_none()
        {
            return;
        }
        self.enforce_watermarks_with_budget_locked(
            &budget,
            oplog,
            state,
            uncommitted_txn_bytes,
            force_full,
        );
    }

    fn enforce_watermarks_with_budget_locked(
        &self,
        budget: &MemoryBudget,
        oplog: &OpLog,
        state: &mut DocState,
        uncommitted_txn_bytes: usize,
        force_full: bool,
    ) {
        if let Some(limit) = budget.max_kv_block_cache_bytes {
            self.kv_block_cache.set_explicit_max_bytes(Some(limit));
        }
        if let Some(limit) = budget.max_decoded_value_cache_bytes {
            state.evict_value_cache_to_bytes(limit);
        }

        let high = if force_full {
            Some(0)
        } else {
            budget.effective_high_watermark()
        };
        let target = if force_full {
            Some(0)
        } else {
            budget.effective_target_watermark()
        };

        if let Some(high) = high {
            let stats = self.memory_stats_locked(oplog, state, uncommitted_txn_bytes);
            if stats.total_bytes <= high && !force_full {
                self.soft_backoff.reset();
            } else if !force_full && self.soft_backoff.backed_off(stats.total_bytes as u64) {
                // The last sweep could not reach the target and usage has barely
                // grown: skip Stage 1's history compaction and full KV flush (with a
                // spill directory, a rewrite of the whole table) on every commit.
            } else {
                #[cfg(test)]
                DOC_STAGE1_PASSES.with(|n| n.set(n.get() + 1));
                // Stage 1: Reclaim caches and soft buffers before touching materialized states.
                state.evict_value_cache_to_bytes(if force_full {
                    0
                } else {
                    budget.max_decoded_value_cache_bytes.unwrap_or(0)
                });
                if (force_full || budget.max_kv_block_cache_bytes.is_none())
                    && !self.kv_block_cache.is_delegating_to_parent()
                {
                    self.kv_block_cache.clear();
                }
                state.reclaim_auxiliary_caches();
                oplog.compact_history();
                #[cfg(not(target_arch = "wasm32"))]
                if force_full || budget.spill_dir.is_some() {
                    oplog.flush_kv();
                }
                #[cfg(target_arch = "wasm32")]
                if force_full {
                    oplog.flush_kv();
                }
                oplog.with_history_cache(|h| h.free_all());
                if force_full {
                    oplog.arena.shrink_spare_capacity();
                }
                self.diff_calculator
                    .lock()
                    .reclaim_cold_caches(if force_full { 0 } else { 4 });
                let _ = self.shrink_undo_managers();

                let target_limit = target.unwrap_or(high);
                let stats_after_stage1 =
                    self.memory_stats_locked(oplog, state, uncommitted_txn_bytes);
                let mut left = stats_after_stage1.total_bytes;
                if stats_after_stage1.total_bytes > target_limit || force_full {
                    // Stage 2: Demote cold materialized container states and compress KV memtables.
                    let need = stats_after_stage1.total_bytes.saturating_sub(target_limit);
                    let target_state_bytes = if force_full {
                        0
                    } else {
                        state.state_estimated_bytes().saturating_sub(need)
                    };
                    state.demote_cold_states(target_state_bytes, &Default::default());
                    state.flush_kv();
                    oplog.flush_kv();
                    if !force_full {
                        left = self
                            .memory_stats_locked(oplog, state, uncommitted_txn_bytes)
                            .total_bytes;
                    }
                }
                if !force_full {
                    self.soft_backoff.record(left <= target_limit, left as u64);
                }
            }
        }

        if !force_full && global_watermarks_active() {
            let active_stats = self.memory_stats_locked(oplog, state, uncommitted_txn_bytes);
            global_pool().enforce_global_watermarks_for_active(
                self,
                oplog,
                state,
                uncommitted_txn_bytes,
                active_stats.total_bytes,
            );
        } else if global_budget_flags() != 0 {
            let _ = self.memory_stats_locked(oplog, state, uncommitted_txn_bytes);
        }
    }

    /// Takes the first failure to write spilled tables to the
    /// [`MemoryBudget::spill_dir`] since the previous call, or `None`.
    ///
    /// A flush that cannot write keeps its tables in memory, so no data is lost,
    /// but the document is no longer (fully) out of core and its memory grows.
    /// Errors from the op log and the state stores are joined with `"; "`.
    /// Always `None` without a spill directory, and on `wasm32`, where spilling
    /// is a no-op.
    pub fn take_spill_error(&self) -> Option<String> {
        let oplog = self.oplog.lock().change_store().take_spill_error();
        let state = self.state.lock().take_spill_error();
        crate::state::container_store::join_spill_errors(oplog, state)
    }

    /// Reclaims caches and demotes cold container states, returning estimated freed bytes.
    pub fn reclaim_memory(&self) -> usize {
        let before = self.memory_stats().total_bytes;
        let uncommitted_txn_bytes = if self.txn.is_held_by_current_thread() {
            0
        } else {
            self.txn
                .lock()
                .as_ref()
                .map_or(0, Transaction::estimated_uncommitted_bytes)
        };
        self.enforce_memory_watermarks_with_txn_bytes(uncommitted_txn_bytes, true);
        let after = self
            .memory_stats_with_txn_bytes(uncommitted_txn_bytes)
            .total_bytes;
        before.saturating_sub(after)
    }

    /// Runs full reclamation and checks whether usage now fits within [`MemoryBudget::max_doc_bytes`].
    pub fn try_reclaim_to_budget(&self) -> LoroResult<DocMemoryStats> {
        let _ = self.reclaim_memory();
        let stats = self.memory_stats();
        if let Some(limit) = self.memory_budget.lock().max_doc_bytes {
            if stats.total_bytes > limit {
                return Err(LoroError::memory_limit_exceeded(
                    MemoryLimitKind::DocBudget,
                    stats.total_bytes,
                    limit,
                ));
            }
        }
        Ok(stats)
    }
}

// Helpers of `LoroDoc::check_apply_diff_creates_no_unknown_container`

fn resolve_container_remap(
    mut id: ContainerID,
    container_remap: &FxHashMap<ContainerID, ContainerID>,
) -> (ContainerID, bool) {
    let mut remapped = false;
    while let Some(rid) = container_remap.get(&id) {
        remapped = true;
        id = rid.clone();
    }
    (id, remapped)
}

fn diff_value_container_id(v: &ValueOrHandler) -> Option<ContainerID> {
    match v {
        ValueOrHandler::Value(LoroValue::Container(id)) => Some(id.clone()),
        ValueOrHandler::Handler(h) => Some(h.id()),
        ValueOrHandler::Value(_) => None,
    }
}

fn inserted_container_ids(
    delta: &crate::event::ListDiff,
) -> impl Iterator<Item = ContainerID> + '_ {
    delta
        .iter()
        .flat_map(|item| match item {
            loro_delta::DeltaItem::Replace { value, .. } => {
                Some(value.iter().filter_map(diff_value_container_id))
            }
            loro_delta::DeltaItem::Retain { .. } => None,
        })
        .flatten()
}

fn diff_has_unknown_value(diff: &crate::event::Diff) -> bool {
    match diff {
        crate::event::Diff::Map(map) => map.updated.values().any(|v| {
            v.value
                .as_ref()
                .and_then(diff_value_container_id)
                .is_some_and(|c| c.is_unknown())
        }),
        crate::event::Diff::List(delta) => inserted_container_ids(delta).any(|c| c.is_unknown()),
        _ => false,
    }
}

/// See `LoroDoc::plan_full_state_batch`.
#[derive(Default)]
struct FullStatePlan {
    /// Batch id -> how the loop applies that full-state entry
    aligned: FxHashMap<ContainerID, PlannedAlignment>,
    /// Targets the loop applies even when they are unreachable before the batch
    always_applied: FxHashSet<ContainerID>,
    /// Batch ids of mergeable children of containers this batch recreates.
    /// They are new and empty when the loop reaches them.
    fresh: FxHashSet<ContainerID>,
}

struct PlannedAlignment {
    /// The container the full state was aligned with
    target: ContainerID,
    /// `None` when there is nothing to do
    edit: Option<crate::event::Diff>,
    /// Children kept by the alignment, whose own entries are full states too
    kept: FxHashSet<ContainerID>,
}

#[cfg(test)]
mod reclamation_work_tests {
    use super::*;

    #[test]
    fn the_linear_ceiling_is_met_by_pages_or_by_heap_within_existing_pages() {
        let max = 1000;
        assert!(
            linear_budget_satisfied(1000, max, 990),
            "pages within the ceiling"
        );
        assert!(
            linear_budget_satisfied(4000, max, 700),
            "pages grew; heap back at 70%"
        );
        assert!(!linear_budget_satisfied(4000, max, 701));
    }
    #[test]
    fn a_two_pass_global_sweep_runs_stage1_once_per_idle_document() {
        let doc = LoroDoc::new_auto_commit();
        doc.get_map("map").insert("value", 1).unwrap();
        doc.commit_then_renew();
        let pool = GlobalMemoryPool::new();
        IDLE_STAGE1_PASSES.with(|n| n.set(0));
        pool.reclaim_idle_docs_to_target(&[RegistryRef::from_doc(&doc)], None, 0, true);
        IDLE_STAGE1_PASSES.with(|n| assert_eq!(n.get(), 1));
        assert_eq!(doc.get_map("map").get("value").unwrap(), LoroValue::I64(1));
    }

    /// The native global estimate recomputes only documents marked dirty, not
    /// every live document (it used to cost ~1 ms per commit with 1,000 idle
    /// documents). Global budgets are off in this test binary, so marks do not
    /// queue documents themselves; the test queues the dirty ones by hand.
    #[test]
    fn the_global_estimate_refreshes_only_documents_marked_dirty() {
        let pool = GlobalMemoryPool::new();
        let docs: Vec<LoroDoc> = (0..40)
            .map(|i| {
                let doc = LoroDoc::new_auto_commit();
                doc.get_map("m").insert("v", i).unwrap();
                doc.commit_then_renew();
                pool.register(doc.pool_doc_id, Arc::downgrade(&doc.inner));
                doc
            })
            .collect();
        DIRTY_REFRESHES.with(|n| n.set(0));
        let _ = pool.estimated_docs_total(None);
        assert_eq!(
            DIRTY_REFRESHES.with(|n| n.get()),
            40,
            "new documents start queued"
        );
        assert!(docs.iter().all(|d| !d.stats_dirty.is_dirty()));
        let refreshes_for_queued_dirty_docs = || {
            for doc in &docs {
                if doc.stats_dirty.is_dirty() {
                    pool.push_dirty(doc.pool_doc_id);
                }
            }
            DIRTY_REFRESHES.with(|n| n.set(0));
            let _ = pool.estimated_docs_total(None);
            DIRTY_REFRESHES.with(|n| n.get())
        };
        assert_eq!(refreshes_for_queued_dirty_docs(), 0);

        let _ = docs[3].get_map("m").get("v");
        assert!(
            docs[3].stats_dirty.is_dirty(),
            "a read takes the state lock"
        );
        let _ = docs[4].get_map("only_registered");
        assert!(
            docs[4].stats_dirty.is_dirty(),
            "a handler registers in the arena"
        );
        docs[5]
            .get_text("t")
            .insert(0, "x", crate::cursor::PosType::Unicode)
            .unwrap();
        let undo = crate::UndoManager::new(&docs[6]);
        refresh_cached_doc_bytes_for_inner(&docs[6].inner);
        docs[6]
            .get_text("t")
            .insert(0, "y", crate::cursor::PosType::Unicode)
            .unwrap();
        docs[6].commit_then_renew();
        let dirty = docs.iter().filter(|d| d.stats_dirty.is_dirty()).count();
        assert_eq!(dirty, 4);
        assert_eq!(refreshes_for_queued_dirty_docs(), 4);
        assert!(docs.iter().all(|d| !d.stats_dirty.is_dirty()));
        drop(undo);
        assert!(
            docs[6].stats_dirty.is_dirty(),
            "undo bytes left with the manager"
        );
        assert_eq!(refreshes_for_queued_dirty_docs(), 1);

        // A busy document keeps its cached value and stays queued.
        let state = docs[7].state.lock();
        assert!(docs[7].stats_dirty.is_dirty());
        assert_eq!(refreshes_for_queued_dirty_docs(), 1);
        assert_eq!(
            pool.dirty_docs.lock().iter().copied().collect::<Vec<_>>(),
            vec![docs[7].pool_doc_id]
        );
        drop(state);
        DIRTY_REFRESHES.with(|n| n.set(0));
        let _ = pool.estimated_docs_total(None);
        assert_eq!(DIRTY_REFRESHES.with(|n| n.get()), 1);
        assert!(pool.dirty_docs.lock().is_empty());

        // (Refreshes update the process-wide pool's totals, so compare per document.)
        for doc in &docs {
            let fresh = fresh_doc_stats_unmarked(&doc.inner).unwrap();
            assert_eq!(doc.cached_doc_bytes.load(Relaxed), fresh.total_bytes);
        }
    }

    /// The per-document Stage 1 (with a spill directory it flushes the whole KV
    /// table) used to run on every commit above the high watermark even when the
    /// previous sweep could not reach the target. It now backs off like the
    /// global sweep.
    #[test]
    fn a_failed_document_soft_sweep_backs_off_until_usage_grows() {
        let doc = LoroDoc::new_auto_commit();
        let text = doc.get_text("t");
        text.insert(0, "first", crate::cursor::PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();
        let passes = || DOC_STAGE1_PASSES.with(|n| n.get());
        let edit = |s: &str| {
            text.insert(0, s, crate::cursor::PosType::Unicode).unwrap();
            doc.commit_then_renew();
        };
        let tiny = || MemoryBudget {
            high_watermark_bytes: Some(1),
            target_watermark_bytes: Some(1),
            ..Default::default()
        };
        let start = passes();
        doc.set_memory_budget(tiny());
        assert_eq!(
            passes() - start,
            1,
            "setting the budget sweeps once and fails"
        );
        for _ in 0..10 {
            edit("x");
        }
        assert_eq!(passes() - start, 1, "no growth: backed off");
        edit(&"grow".repeat(256 * 1024));
        assert_eq!(passes() - start, 2, "usage grew: swept again");
        doc.set_memory_budget(tiny());
        assert_eq!(passes() - start, 3, "a new budget clears the backoff");
        let _ = doc.reclaim_memory();
        assert_eq!(passes() - start, 4, "explicit reclamation never backs off");
        assert_eq!(text.len_unicode(), "first".len() + 10 + 4 * 256 * 1024);
    }

    /// A soft sweep that cannot reach its target is not repeated on every
    /// commit; it is retried after usage grows or after a bounded number of
    /// skipped checks. (Hard admission, `check_global_budget_with_active`, never
    /// reads the backoff.)
    #[test]
    fn a_failed_global_soft_sweep_backs_off_until_usage_grows() {
        let pool = GlobalMemoryPool::new();
        *pool.budget.lock() = GlobalMemoryBudget {
            high_watermark_bytes: Some(1),
            target_watermark_bytes: Some(1),
            ..Default::default()
        };
        let doc = LoroDoc::new_auto_commit();
        pool.register(doc.pool_doc_id, Arc::downgrade(&doc.inner));
        let text = doc.get_text("t");
        let edit_and_check = |s: &str| {
            text.insert(0, s, crate::cursor::PosType::Unicode).unwrap();
            doc.commit_then_renew();
            let oplog = doc.oplog.lock();
            let mut state = doc.state.lock();
            let bytes = doc.memory_stats_locked(&oplog, &state, 0).total_bytes;
            // `cached_doc_bytes` is shared with the process-wide pool; start this
            // private pool's total from the active document alone.
            doc.cached_doc_bytes.store(0, Relaxed);
            pool.total_cached_doc_bytes.store(0, Relaxed);
            pool.enforce_global_watermarks_for_active(&doc, &oplog, &mut state, 0, bytes);
        };
        let passes = || ACTIVE_FALLBACK_PASSES.with(|n| n.get());
        let start = passes();
        edit_and_check("first");
        assert_eq!(passes() - start, 1);
        for _ in 0..10 {
            edit_and_check("x");
        }
        assert_eq!(passes() - start, 1, "no growth: backed off");

        edit_and_check(&"grow".repeat(256 * 1024));
        assert_eq!(passes() - start, 2, "usage grew: swept again");

        for _ in 0..SOFT_BACKOFF_MAX_SKIPS + 1 {
            edit_and_check("y");
        }
        assert_eq!(passes() - start, 3, "retried after the skip limit");
    }

    /// The registry's temporary reference must not make the last handle skip its
    /// implicit commit; the registry's own release must not commit either.
    #[test]
    fn last_handle_commits_while_the_registry_holds_a_reference() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        let doc = LoroDoc::new_auto_commit();
        let updates = Arc::new(AtomicUsize::new(0));
        let counter = updates.clone();
        doc.subscribe_local_update(Box::new(move |_| {
            counter.fetch_add(1, SeqCst);
            true
        }))
        .detach();
        doc.get_text("t")
            .insert(0, "x", crate::cursor::PosType::Unicode)
            .unwrap();
        let weak = Arc::downgrade(&doc.inner);
        let held = RegistryRef::from_doc(&doc);
        let other = doc.clone();
        drop(doc);
        assert_eq!(updates.load(SeqCst), 0, "another handle is still alive");
        drop(other);
        assert_eq!(updates.load(SeqCst), 1, "the last handle commits");
        assert!(weak.upgrade().is_some());
        drop(held);
        assert_eq!(updates.load(SeqCst), 1);
        assert!(weak.upgrade().is_none());
    }

    /// Arming the hard scope from `set_global_memory_budget` used to create a
    /// temporary `LoroDoc` while holding the transaction lock.
    #[test]
    fn arming_a_global_scope_does_not_create_a_handle() {
        let doc = LoroDoc::new_auto_commit();
        let before = Arc::strong_count(&doc.inner);
        let held = RegistryRef::from_doc(&doc);
        let mut guard = held.txn.try_lock().unwrap();
        held.arm_budget_scope_on_txn(guard.as_mut().unwrap());
        assert_eq!(Arc::strong_count(&doc.inner), before + 1);
        drop(guard);
        drop(held);
        assert_eq!(Arc::strong_count(&doc.inner), before);
    }
}
