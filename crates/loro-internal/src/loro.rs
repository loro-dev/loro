use crate::change::ChangeRef;
pub use crate::encoding::ExportMode;
pub use crate::state::analyzer::{ContainerAnalysisInfo, DocAnalysis};
pub(crate) use crate::LoroDocInner;
use crate::{
    arena::SharedArena,
    change::Timestamp,
    configure::{Configure, DefaultRandom, SecureRandomGenerator, StyleConfig},
    container::{
        idx::ContainerIdx, list::list_op::InnerListOp, richtext::config::StyleConfigMap,
        IntoContainerId,
    },
    cursor::{AbsolutePosition, CannotFindRelativePosition, Cursor, PosQueryResult},
    dag::{Dag, DagUtils},
    diff_calc::DiffCalculator,
    encoding::{
        self, decode_snapshot, export_fast_snapshot, export_fast_updates,
        export_fast_updates_in_range, export_shallow_snapshot, export_snapshot, export_snapshot_at,
        export_state_only_snapshot,
        json_schema::{encode_change_to_json, json::JsonSchema},
        parse_header_and_body, EncodeMode, ImportBlobMetadata, ImportStatus, ParsedHeaderAndBody,
    },
    event::{str_to_path, EventTriggerKind, Index, InternalDocDiff},
    handler::{Handler, MovableListHandler, TextHandler, TreeHandler, ValueOrHandler},
    id::PeerID,
    json::JsonChange,
    op::InnerContent,
    oplog::{loro_dag::FrontiersNotIncluded, OpLog},
    state::DocState,
    subscription::{LocalUpdateCallback, Observer, Subscriber},
    undo::DiffBatch,
    utils::subscription::{SubscriberSetWithQueue, Subscription},
    version::{shrink_frontiers, Frontiers, ImVersionVector, VersionRange, VersionVectorDiff},
    ChangeMeta, DocDiff, HandlerTrait, InternalString, ListHandler, LoroDoc, LoroError, MapHandler,
    VersionVector,
};
use either::Either;
use fxhash::{FxHashMap, FxHashSet};
use loro_common::{
    ContainerID, ContainerType, HasIdSpan, HasLamportSpan, IdSpan, LoroEncodeError, LoroResult,
    LoroValue, ID,
};
use rle::HasLength;
use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::{hash_map::Entry, BinaryHeap},
    ops::ControlFlow,
    sync::{
        atomic::{
            AtomicBool,
            Ordering::{Acquire, Release},
        },
        Arc, Mutex,
    },
};
use tracing::{debug_span, info, info_span, instrument, warn};

impl Default for LoroDoc {
    fn default() -> Self {
        Self::new()
    }
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

impl LoroDoc {
    pub fn new() -> Self {
        let oplog = OpLog::new();
        let arena = oplog.arena.clone();
        let config: Configure = oplog.configure.clone();
        let global_txn = Arc::new(Mutex::new(None));
        let inner = Arc::new_cyclic(|w| {
            let state = DocState::new_arc(w.clone(), arena.clone(), config.clone());
            LoroDocInner {
                oplog: Arc::new(Mutex::new(oplog)),
                state,
                config,
                detached: AtomicBool::new(false),
                auto_commit: AtomicBool::new(false),
                observer: Arc::new(Observer::new(arena.clone())),
                diff_calculator: Arc::new(Mutex::new(DiffCalculator::new(true))),
                txn: global_txn,
                arena,
                local_update_subs: SubscriberSetWithQueue::new(),
                peer_id_change_subs: SubscriberSetWithQueue::new(),
            }
        });
        Self { inner }
    }

    pub fn fork(&self) -> Self {
        if self.is_detached() {
            return self.fork_at(&self.state_frontiers());
        }

        let options = self.commit_then_stop();
        let snapshot = encoding::fast_snapshot::encode_snapshot_inner(self);
        let doc = Self::new();
        encoding::fast_snapshot::decode_snapshot_inner(snapshot, &doc).unwrap();
        doc.set_config(&self.config);
        if self.auto_commit.load(std::sync::atomic::Ordering::Relaxed) {
            doc.start_auto_commit();
        }
        self.renew_txn_if_auto_commit(options);
        doc
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
    ///   concurrent operations with the same PeerID are not possible.
    /// - On detached mode, importing will not change the state of the document.
    ///   It also doesn't change the version of the [DocState]. The changes will be
    ///   recorded into [OpLog] only. You need to call `checkout` to make it take effect.
    pub fn set_detached_editing(&self, enable: bool) {
        self.config.set_detached_editing(enable);
        if enable && self.is_detached() {
            let options = self.commit_then_stop();
            self.renew_peer_id();
            self.renew_txn_if_auto_commit(options);
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
        let next_id = self.oplog.try_lock().unwrap().next_id(peer);
        if self.auto_commit.load(Acquire) {
            let doc_state = self.state.try_lock().unwrap();
            doc_state
                .peer
                .store(peer, std::sync::atomic::Ordering::Relaxed);
            drop(doc_state);

            let txn = self.txn.try_lock().unwrap().take();
            if let Some(txn) = txn {
                txn.commit().unwrap();
            }

            let new_txn = self.txn().unwrap();
            self.txn.try_lock().unwrap().replace(new_txn);
            self.peer_id_change_subs.emit(&(), next_id);
            return Ok(());
        }

        let doc_state = self.state.try_lock().unwrap();
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
        let peer_id = DefaultRandom.next_u64();
        self.set_peer_id(peer_id).unwrap();
    }

    /// Commit the cumulative auto commit transaction.
    /// This method only has effect when `auto_commit` is true.
    ///
    /// Afterwards, the users need to call `self.renew_txn_after_commit()` to resume the continuous transaction.
    ///
    /// It only returns Some(options_of_the_empty_txn) when the txn is empty
    #[inline]
    #[must_use]
    pub fn commit_then_stop(&self) -> Option<CommitOptions> {
        self.commit_with(CommitOptions::new().immediate_renew(false))
    }

    /// Commit the cumulative auto commit transaction.
    /// It will start the next one immediately
    ///
    /// It only returns Some(options_of_the_empty_txn) when the txn is empty
    #[inline]
    pub fn commit_then_renew(&self) -> Option<CommitOptions> {
        self.commit_with(CommitOptions::new().immediate_renew(true))
    }

    /// Commit the cumulative auto commit transaction.
    /// This method only has effect when `auto_commit` is true.
    /// If `immediate_renew` is true, a new transaction will be created after the old one is committed
    ///
    /// It only returns Some(options_of_the_empty_txn) when the txn is empty
    #[instrument(skip_all)]
    pub fn commit_with(&self, config: CommitOptions) -> Option<CommitOptions> {
        if !self.auto_commit.load(Acquire) {
            // if not auto_commit, nothing should happen
            // because the global txn is not used
            return None;
        }

        let mut txn_guard = self.txn.try_lock().unwrap();
        let txn = txn_guard.take();
        drop(txn_guard);
        let mut txn = txn?;
        let on_commit = txn.take_on_commit();
        if let Some(origin) = config.origin {
            txn.set_origin(origin);
        }

        if let Some(timestamp) = config.timestamp {
            txn.set_timestamp(timestamp);
        }

        if let Some(msg) = config.commit_msg.as_ref() {
            txn.set_msg(Some(msg.clone()));
        }

        let id_span = txn.id_span();
        let options = txn.commit().unwrap();
        if config.immediate_renew {
            let mut txn_guard = self.txn.try_lock().unwrap();
            assert!(self.can_edit());
            let mut t = self.txn().unwrap();
            if let Some(options) = options.as_ref() {
                t.set_options(options.clone());
            }
            *txn_guard = Some(t);
        }

        if let Some(on_commit) = on_commit {
            on_commit(&self.state, &self.oplog, id_span);
        }

        options
    }

    /// Set the commit message of the next commit
    pub fn set_next_commit_message(&self, message: &str) {
        let mut binding = self.txn.try_lock().unwrap();
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
        let mut txn = self.txn.try_lock().unwrap();
        if let Some(txn) = txn.as_mut() {
            txn.set_origin(origin.into());
        }
    }

    /// Set the timestamp of the next commit
    pub fn set_next_commit_timestamp(&self, timestamp: Timestamp) {
        let mut txn = self.txn.try_lock().unwrap();
        if let Some(txn) = txn.as_mut() {
            txn.set_timestamp(timestamp);
        }
    }

    /// Set the options of the next commit
    pub fn set_next_commit_options(&self, options: CommitOptions) {
        let mut txn = self.txn.try_lock().unwrap();
        if let Some(txn) = txn.as_mut() {
            txn.set_options(options);
        }
    }

    /// Clear the options of the next commit
    pub fn clear_next_commit_options(&self) {
        let mut txn = self.txn.try_lock().unwrap();
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
        self.config.text_style_config.try_write().unwrap().map = text_style.map;
    }

    #[inline]
    pub fn config_default_text_style(&self, text_style: Option<StyleConfig>) {
        self.config
            .text_style_config
            .try_write()
            .unwrap()
            .default_style = text_style;
    }
    pub fn from_snapshot(bytes: &[u8]) -> LoroResult<Self> {
        let doc = Self::new();
        let ParsedHeaderAndBody { mode, body, .. } = parse_header_and_body(bytes, true)?;
        if mode.is_snapshot() {
            decode_snapshot(&doc, mode, body)?;
            Ok(doc)
        } else {
            Err(LoroError::DecodeError(
                "Invalid encode mode".to_string().into(),
            ))
        }
    }

    /// Is the document empty? (no ops)
    #[inline(always)]
    pub fn can_reset_with_snapshot(&self) -> bool {
        let oplog = self.oplog.try_lock().unwrap();
        if oplog.batch_importing {
            return false;
        }

        if self.is_detached() {
            return false;
        }

        oplog.is_empty() && self.state.try_lock().unwrap().can_import_snapshot()
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
            .try_lock()
            .unwrap()
            .peer
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    #[inline(always)]
    pub fn detach(&self) {
        let options = self.commit_then_stop();
        self.set_detached(true);
        self.renew_txn_if_auto_commit(options);
    }

    #[inline(always)]
    pub fn attach(&self) {
        self.checkout_to_latest()
    }

    /// Get the timestamp of the current state.
    /// It's the last edit time of the [DocState].
    pub fn state_timestamp(&self) -> Timestamp {
        let f = &self.state.try_lock().unwrap().frontiers;
        self.oplog.try_lock().unwrap().get_timestamp_of_version(f)
    }

    #[inline(always)]
    pub fn app_state(&self) -> &Arc<Mutex<DocState>> {
        &self.state
    }

    #[inline]
    pub fn get_state_deep_value(&self) -> LoroValue {
        self.state.try_lock().unwrap().get_deep_value()
    }

    #[inline(always)]
    pub fn oplog(&self) -> &Arc<Mutex<OpLog>> {
        &self.oplog
    }

    pub fn export_from(&self, vv: &VersionVector) -> Vec<u8> {
        let options = self.commit_then_stop();
        let ans = self.oplog.try_lock().unwrap().export_from(vv);
        self.renew_txn_if_auto_commit(options);
        ans
    }

    #[inline(always)]
    pub fn import(&self, bytes: &[u8]) -> Result<ImportStatus, LoroError> {
        let s = debug_span!("import", peer = self.peer_id());
        let _e = s.enter();
        self.import_with(bytes, Default::default())
    }

    #[inline]
    pub fn import_with(
        &self,
        bytes: &[u8],
        origin: InternalString,
    ) -> Result<ImportStatus, LoroError> {
        let options = self.commit_then_stop();
        let ans = self._import_with(bytes, origin);
        self.renew_txn_if_auto_commit(options);
        ans
    }

    #[tracing::instrument(skip_all)]
    fn _import_with(
        &self,
        bytes: &[u8],
        origin: InternalString,
    ) -> Result<ImportStatus, LoroError> {
        ensure_cov::notify_cov("loro_internal::import");
        let parsed = parse_header_and_body(bytes, true)?;
        info!("Importing with mode={:?}", &parsed.mode);
        let result = match parsed.mode {
            EncodeMode::OutdatedRle => {
                if self.state.try_lock().unwrap().is_in_txn() {
                    return Err(LoroError::ImportWhenInTxn);
                }

                let s = tracing::span!(
                    tracing::Level::INFO,
                    "Import updates ",
                    peer = self.peer_id()
                );
                let _e = s.enter();
                self.update_oplog_and_apply_delta_to_state_if_needed(
                    |oplog| oplog.decode(parsed),
                    origin,
                )
            }
            EncodeMode::OutdatedSnapshot => {
                if self.can_reset_with_snapshot() {
                    tracing::info!("Init by snapshot {}", self.peer_id());
                    decode_snapshot(self, parsed.mode, parsed.body)
                } else {
                    self.update_oplog_and_apply_delta_to_state_if_needed(
                        |oplog| oplog.decode(parsed),
                        origin,
                    )
                }
            }
            EncodeMode::FastSnapshot => {
                if self.can_reset_with_snapshot() {
                    ensure_cov::notify_cov("loro_internal::import::snapshot");
                    tracing::info!("Init by fast snapshot {}", self.peer_id());
                    decode_snapshot(self, parsed.mode, parsed.body)
                } else {
                    self.update_oplog_and_apply_delta_to_state_if_needed(
                        |oplog| oplog.decode(parsed),
                        origin,
                    )

                    // let new_doc = LoroDoc::new();
                    // new_doc.import(bytes)?;
                    // let updates = new_doc.export_from(&self.oplog_vv());
                    // return self.import_with(updates.as_slice(), origin);
                }
            }
            EncodeMode::FastUpdates => self.update_oplog_and_apply_delta_to_state_if_needed(
                |oplog| oplog.decode(parsed),
                origin,
            ),
            EncodeMode::Auto => {
                unreachable!()
            }
        };

        self.emit_events();
        result
    }

    #[tracing::instrument(skip_all)]
    pub(crate) fn update_oplog_and_apply_delta_to_state_if_needed(
        &self,
        f: impl FnOnce(&mut OpLog) -> Result<ImportStatus, LoroError>,
        origin: InternalString,
    ) -> Result<ImportStatus, LoroError> {
        let mut oplog = self.oplog.try_lock().unwrap();
        if !self.is_detached() {
            let old_vv = oplog.vv().clone();
            let old_frontiers = oplog.frontiers().clone();
            let result = f(&mut oplog);
            if &old_vv != oplog.vv() {
                let mut diff = DiffCalculator::new(false);
                let (diff, diff_mode) = diff.calc_diff_internal(
                    &oplog,
                    &old_vv,
                    &old_frontiers,
                    oplog.vv(),
                    oplog.dag.get_frontiers(),
                    None,
                );
                let mut state = self.state.try_lock().unwrap();
                state.apply_diff(
                    InternalDocDiff {
                        origin,
                        diff: (diff).into(),
                        by: EventTriggerKind::Import,
                        new_version: Cow::Owned(oplog.frontiers().clone()),
                    },
                    diff_mode,
                );
            }
            result
        } else {
            f(&mut oplog)
        }
    }

    fn emit_events(&self) {
        // we should not hold the lock when emitting events
        let events = {
            let mut state = self.state.try_lock().unwrap();
            state.take_events()
        };
        for event in events {
            self.observer.emit(event);
        }
    }

    pub(crate) fn drop_pending_events(&self) -> Vec<DocDiff> {
        let mut state = self.state.try_lock().unwrap();
        state.take_events()
    }

    #[instrument(skip_all)]
    pub fn export_snapshot(&self) -> Result<Vec<u8>, LoroEncodeError> {
        if self.is_shallow() {
            return Err(LoroEncodeError::ShallowSnapshotIncompatibleWithOldFormat);
        }
        let options = self.commit_then_stop();
        let ans = export_snapshot(self);
        self.renew_txn_if_auto_commit(options);
        Ok(ans)
    }

    /// Import the json schema updates.
    ///
    /// only supports backward compatibility but not forward compatibility.
    #[tracing::instrument(skip_all)]
    pub fn import_json_updates<T: TryInto<JsonSchema>>(&self, json: T) -> LoroResult<ImportStatus> {
        let json = json.try_into().map_err(|_| LoroError::InvalidJsonSchema)?;
        let options = self.commit_then_stop();
        let result = self.update_oplog_and_apply_delta_to_state_if_needed(
            |oplog| crate::encoding::json_schema::import_json(oplog, json),
            Default::default(),
        );
        self.emit_events();
        self.renew_txn_if_auto_commit(options);
        result
    }

    pub fn export_json_updates(
        &self,
        start_vv: &VersionVector,
        end_vv: &VersionVector,
        with_peer_compression: bool,
    ) -> JsonSchema {
        let options = self.commit_then_stop();
        let oplog = self.oplog.try_lock().unwrap();
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

        let json = crate::encoding::json_schema::export_json(
            &oplog,
            start_vv,
            end_vv,
            with_peer_compression,
        );
        drop(oplog);
        self.renew_txn_if_auto_commit(options);
        json
    }

    pub fn export_json_in_id_span(&self, id_span: IdSpan) -> Vec<JsonChange> {
        let options = self.commit_then_stop();
        let oplog = self.oplog.try_lock().unwrap();
        let json = crate::encoding::json_schema::export_json_in_id_span(&oplog, id_span);
        drop(oplog);
        self.renew_txn_if_auto_commit(options);
        json
    }

    /// Get the version vector of the current OpLog
    #[inline]
    pub fn oplog_vv(&self) -> VersionVector {
        self.oplog.try_lock().unwrap().vv().clone()
    }

    /// Get the version vector of the current [DocState]
    #[inline]
    pub fn state_vv(&self) -> VersionVector {
        let f = &self.state.try_lock().unwrap().frontiers;
        self.oplog
            .try_lock()
            .unwrap()
            .dag
            .frontiers_to_vv(f)
            .unwrap()
    }

    pub fn get_by_path(&self, path: &[Index]) -> Option<ValueOrHandler> {
        let value: LoroValue = self.state.try_lock().unwrap().get_value_by_path(path)?;
        if let LoroValue::Container(c) = value {
            Some(ValueOrHandler::Handler(Handler::new_attached(
                c.clone(),
                self.inner.clone(),
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
        let txn = self.txn.try_lock().unwrap();
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
                .unwrap_or_else(|| self.oplog.try_lock().unwrap().get_timestamp_for_next_txn()),
            commit_msg: txn.msg(),
            ops: ops_,
            lamport: txn.lamport(),
        };
        let json = encode_change_to_json(change, arena);
        Some(json)
    }

    #[inline]
    pub fn get_handler(&self, id: &ContainerID) -> Option<Handler> {
        if self.has_container(id) {
            Some(Handler::new_attached(id.clone(), self.inner.clone()))
        } else {
            None
        }
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_text<I: IntoContainerId>(&self, id: I) -> TextHandler {
        let id = id.into_container_id(&self.arena, ContainerType::Text);
        assert!(self.has_container(&id));
        Handler::new_attached(id, self.inner.clone())
            .into_text()
            .unwrap()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_list<I: IntoContainerId>(&self, id: I) -> ListHandler {
        let id = id.into_container_id(&self.arena, ContainerType::List);
        assert!(self.has_container(&id));
        Handler::new_attached(id, self.inner.clone())
            .into_list()
            .unwrap()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_movable_list<I: IntoContainerId>(&self, id: I) -> MovableListHandler {
        let id = id.into_container_id(&self.arena, ContainerType::MovableList);
        assert!(self.has_container(&id));
        Handler::new_attached(id, self.inner.clone())
            .into_movable_list()
            .unwrap()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_map<I: IntoContainerId>(&self, id: I) -> MapHandler {
        let id = id.into_container_id(&self.arena, ContainerType::Map);
        assert!(self.has_container(&id));
        Handler::new_attached(id, self.inner.clone())
            .into_map()
            .unwrap()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_tree<I: IntoContainerId>(&self, id: I) -> TreeHandler {
        let id = id.into_container_id(&self.arena, ContainerType::Tree);
        assert!(self.has_container(&id));
        Handler::new_attached(id, self.inner.clone())
            .into_tree()
            .unwrap()
    }

    #[cfg(feature = "counter")]
    pub fn get_counter<I: IntoContainerId>(
        &self,
        id: I,
    ) -> crate::handler::counter::CounterHandler {
        let id = id.into_container_id(&self.arena, ContainerType::Counter);
        assert!(self.has_container(&id));
        Handler::new_attached(id, self.inner.clone())
            .into_counter()
            .unwrap()
    }

    #[must_use]
    pub fn has_container(&self, id: &ContainerID) -> bool {
        if id.is_root() {
            return true;
        }

        let exist = self.state.try_lock().unwrap().does_container_exist(id);
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
    #[instrument(level = "info", skip_all)]
    pub fn undo_internal(
        &self,
        id_span: IdSpan,
        container_remap: &mut FxHashMap<ContainerID, ContainerID>,
        post_transform_base: Option<&DiffBatch>,
        before_diff: &mut dyn FnMut(&DiffBatch),
    ) -> LoroResult<CommitWhenDrop> {
        if !self.can_edit() {
            return Err(LoroError::EditWhenDetached);
        }

        let options = self.commit_then_stop();
        if !self
            .oplog()
            .try_lock()
            .unwrap()
            .vv()
            .includes_id(id_span.id_last())
        {
            self.renew_txn_if_auto_commit(options);
            return Err(LoroError::UndoInvalidIdSpan(id_span.id_last()));
        }

        let (was_recording, latest_frontiers) = {
            let mut state = self.state.try_lock().unwrap();
            let was_recording = state.is_recording();
            state.stop_and_clear_recording();
            (was_recording, state.frontiers.clone())
        };

        let spans = self
            .oplog
            .try_lock()
            .unwrap()
            .split_span_based_on_deps(id_span);
        let diff = crate::undo::undo(
            spans,
            match post_transform_base {
                Some(d) => Either::Right(d),
                None => Either::Left(&latest_frontiers),
            },
            |from, to| {
                self.checkout_without_emitting(from, false).unwrap();
                self.state.try_lock().unwrap().start_recording();
                self.checkout_without_emitting(to, false).unwrap();
                let mut state = self.state.try_lock().unwrap();
                let e = state.take_events();
                state.stop_and_clear_recording();
                DiffBatch::new(e)
            },
            before_diff,
        );

        // println!("\nundo_internal: diff: {:?}", diff);
        // println!("container remap: {:?}", container_remap);

        self.checkout_without_emitting(&latest_frontiers, false)?;
        self.set_detached(false);
        if was_recording {
            self.state.try_lock().unwrap().start_recording();
        }
        self.start_auto_commit();
        // Try applying the diff, but ignore the error if it happens.
        // MovableList's undo behavior is too tricky to handle in a collaborative env
        // so in edge cases this may be an Error
        if let Err(e) = self._apply_diff(diff, container_remap, true) {
            warn!("Undo Failed {:?}", e);
        }

        if let Some(options) = options {
            self.set_next_commit_options(options);
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
        let diff = self.diff(&f, target)?;
        self._apply_diff(diff, &mut Default::default(), false)
    }

    /// Calculate the diff between two versions so that apply diff on a will make the state same as b.
    ///
    /// NOTE: This method will make the doc enter the **detached mode**.
    // FIXME: This method needs testing (no event should be emitted during processing this)
    pub fn diff(&self, a: &Frontiers, b: &Frontiers) -> LoroResult<DiffBatch> {
        {
            // check whether a and b are valid
            let oplog = self.oplog.try_lock().unwrap();
            for id in a.iter() {
                if !oplog.dag.contains(id) {
                    return Err(LoroError::FrontiersNotFound(id));
                }
            }
            for id in b.iter() {
                if !oplog.dag.contains(id) {
                    return Err(LoroError::FrontiersNotFound(id));
                }
            }
        }

        let options = self.commit_then_stop();
        let was_detached = self.is_detached();
        let old_frontiers = self.state_frontiers();
        let was_recording = {
            let mut state = self.state.try_lock().unwrap();
            let is_recording = state.is_recording();
            state.stop_and_clear_recording();
            is_recording
        };
        self.checkout_without_emitting(a, true).unwrap();
        self.state.try_lock().unwrap().start_recording();
        self.checkout_without_emitting(b, true).unwrap();
        let e = {
            let mut state = self.state.try_lock().unwrap();
            let e = state.take_events();
            state.stop_and_clear_recording();
            e
        };
        self.checkout_without_emitting(&old_frontiers, false)
            .unwrap();
        if !was_detached {
            self.set_detached(false);
            self.renew_txn_if_auto_commit(options);
        }
        if was_recording {
            self.state.try_lock().unwrap().start_recording();
        }
        Ok(DiffBatch::new(e))
    }

    /// Apply a diff to the current state.
    #[inline(always)]
    pub fn apply_diff(&self, diff: DiffBatch) -> LoroResult<()> {
        self._apply_diff(diff, &mut Default::default(), true)
    }

    /// Apply a diff to the current state.
    ///
    /// This method will not recreate containers with the same [ContainerID]s.
    /// While this can be convenient in certain cases, it can break several internal invariants:
    ///
    /// 1. Each container should appear only once in the document. Allowing containers with the same ID
    ///    would result in multiple instances of the same container in the document.
    /// 2. Unreachable containers should be removable from the state when necessary.
    ///
    /// However, the diff may contain operations that depend on container IDs.
    /// Therefore, users need to provide a `container_remap` to record and retrieve the container ID remapping.
    pub(crate) fn _apply_diff(
        &self,
        diff: DiffBatch,
        container_remap: &mut FxHashMap<ContainerID, ContainerID>,
        skip_unreachable: bool,
    ) -> LoroResult<()> {
        if !self.can_edit() {
            return Err(LoroError::EditWhenDetached);
        }

        let mut ans: LoroResult<()> = Ok(());
        let mut missing_containers: Vec<ContainerID> = Vec::new();
        for (mut id, diff) in diff.into_iter() {
            info!(
                "id: {:?} diff: {:?} remap: {:?}",
                &id, &diff, container_remap
            );
            let mut remapped = false;
            while let Some(rid) = container_remap.get(&id) {
                remapped = true;
                id = rid.clone();
            }

            if matches!(&id, ContainerID::Normal { .. }) && self.arena.id_to_idx(&id).is_none() {
                missing_containers.push(id);
                continue;
            }

            if skip_unreachable && !remapped && !self.state.try_lock().unwrap().get_reachable(&id) {
                continue;
            }

            let Some(h) = self.get_handler(&id) else {
                return Err(LoroError::ContainersNotFound {
                    containers: Box::new(vec![id]),
                });
            };
            if let Err(e) = h.apply_diff(diff, container_remap) {
                ans = Err(e);
            }
        }

        if !missing_containers.is_empty() {
            return Err(LoroError::ContainersNotFound {
                containers: Box::new(missing_containers),
            });
        }

        ans
    }

    /// This is for debugging purpose. It will travel the whole oplog
    #[inline]
    pub fn diagnose_size(&self) {
        self.oplog().try_lock().unwrap().diagnose_size();
    }

    #[inline]
    pub fn oplog_frontiers(&self) -> Frontiers {
        self.oplog().try_lock().unwrap().frontiers().clone()
    }

    #[inline]
    pub fn state_frontiers(&self) -> Frontiers {
        self.state.try_lock().unwrap().frontiers.clone()
    }

    /// - Ordering::Less means self is less than target or parallel
    /// - Ordering::Equal means versions equal
    /// - Ordering::Greater means self's version is greater than target
    #[inline]
    pub fn cmp_with_frontiers(&self, other: &Frontiers) -> Ordering {
        self.oplog().try_lock().unwrap().cmp_with_frontiers(other)
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
        self.oplog().try_lock().unwrap().cmp_frontiers(a, b)
    }

    pub fn subscribe_root(&self, callback: Subscriber) -> Subscription {
        let mut state = self.state.try_lock().unwrap();
        if !state.is_recording() {
            state.start_recording();
        }

        self.observer.subscribe_root(callback)
    }

    pub fn subscribe(&self, container_id: &ContainerID, callback: Subscriber) -> Subscription {
        let mut state = self.state.try_lock().unwrap();
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

    // PERF: opt
    #[tracing::instrument(skip_all)]
    pub fn import_batch(&self, bytes: &[Vec<u8>]) -> LoroResult<ImportStatus> {
        if bytes.is_empty() {
            return Ok(ImportStatus::default());
        }

        if bytes.len() == 1 {
            return self.import(&bytes[0]);
        }

        let mut success = VersionRange::default();
        let mut pending = VersionRange::default();
        let mut meta_arr = bytes
            .iter()
            .map(|b| Ok((LoroDoc::decode_import_blob_meta(b, false)?, b)))
            .collect::<LoroResult<Vec<(ImportBlobMetadata, &Vec<u8>)>>>()?;
        meta_arr.sort_by(|a, b| {
            a.0.mode
                .cmp(&b.0.mode)
                .then(b.0.change_num.cmp(&a.0.change_num))
        });

        let options = self.commit_then_stop();
        let is_detached = self.is_detached();
        self.detach();
        self.oplog.try_lock().unwrap().batch_importing = true;
        let mut err = None;
        for (_meta, data) in meta_arr {
            match self.import(data) {
                Ok(s) => {
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

                    if let Some(p) = s.pending.as_ref() {
                        for (&peer, &(start, end)) in p.iter() {
                            match pending.0.entry(peer) {
                                Entry::Occupied(mut e) => {
                                    e.get_mut().0 = start.min(e.get().0);
                                    e.get_mut().1 = end.min(e.get().1);
                                }
                                Entry::Vacant(e) => {
                                    e.insert((start, end));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    err = Some(e);
                }
            }
        }

        let mut oplog = self.oplog.try_lock().unwrap();
        oplog.batch_importing = false;
        drop(oplog);

        if !is_detached {
            self.checkout_to_latest();
        }

        self.renew_txn_if_auto_commit(options);
        if let Some(err) = err {
            return Err(err);
        }

        Ok(ImportStatus {
            success,
            pending: if pending.is_empty() {
                None
            } else {
                Some(pending)
            },
        })
    }

    /// Get shallow value of the document.
    #[inline]
    pub fn get_value(&self) -> LoroValue {
        self.state.try_lock().unwrap().get_value()
    }

    /// Get deep value of the document.
    #[inline]
    pub fn get_deep_value(&self) -> LoroValue {
        self.state.try_lock().unwrap().get_deep_value()
    }

    /// Get deep value of the document with container id
    #[inline]
    pub fn get_deep_value_with_id(&self) -> LoroValue {
        self.state.try_lock().unwrap().get_deep_value_with_id()
    }

    pub fn checkout_to_latest(&self) {
        let options = self.commit_then_renew();
        if !self.is_detached() {
            return;
        }

        tracing::info_span!("CheckoutToLatest", peer = self.peer_id()).in_scope(|| {
            let f = self.oplog_frontiers();
            let this = &self;
            let frontiers = &f;
            this.checkout_without_emitting(frontiers, false).unwrap(); // we don't need to shrink frontiers
                                                                       // because oplog's frontiers are already shrinked
            this.emit_events();
            if this.config.detached_editing() {
                this.renew_peer_id();
            }

            self.set_detached(false);
            self.renew_txn_if_auto_commit(options);
        });
    }

    /// Checkout [DocState] to a specific version.
    ///
    /// This will make the current [DocState] detached from the latest version of [OpLog].
    /// Any further import will not be reflected on the [DocState], until user call [LoroDoc::attach()]
    pub fn checkout(&self, frontiers: &Frontiers) -> LoroResult<()> {
        let options = self.checkout_without_emitting(frontiers, true)?;
        self.emit_events();
        if self.config.detached_editing() {
            self.renew_peer_id();
            self.renew_txn_if_auto_commit(options);
        }

        Ok(())
    }

    #[instrument(level = "info", skip(self))]
    pub(crate) fn checkout_without_emitting(
        &self,
        frontiers: &Frontiers,
        to_shrink_frontiers: bool,
    ) -> Result<Option<CommitOptions>, LoroError> {
        let mut options = None;
        let had_txn = self.txn.try_lock().unwrap().is_some();
        if had_txn {
            options = self.commit_then_stop();
        }
        let from_frontiers = self.state_frontiers();
        info!(
            "checkout from={:?} to={:?} cur_vv={:?}",
            from_frontiers,
            frontiers,
            self.oplog_vv()
        );

        if &from_frontiers == frontiers {
            if had_txn {
                self.renew_txn_if_auto_commit(options);
            }
            return Ok(None);
        }

        let oplog = self.oplog.try_lock().unwrap();
        if oplog.dag.is_before_shallow_root(frontiers) {
            drop(oplog);
            if had_txn {
                self.renew_txn_if_auto_commit(options);
            }
            return Err(LoroError::SwitchToVersionBeforeShallowRoot);
        }

        let frontiers = if to_shrink_frontiers {
            shrink_frontiers(frontiers, &oplog.dag)
                .map_err(|_| LoroError::SwitchToVersionBeforeShallowRoot)?
        } else {
            frontiers.clone()
        };
        if from_frontiers == frontiers {
            drop(oplog);
            if had_txn {
                self.renew_txn_if_auto_commit(options);
            }
            return Ok(None);
        }

        let mut state = self.state.try_lock().unwrap();
        let mut calc = self.diff_calculator.try_lock().unwrap();
        for i in frontiers.iter() {
            if !oplog.dag.contains(i) {
                drop(oplog);
                drop(state);
                if had_txn {
                    self.renew_txn_if_auto_commit(options);
                }
                return Err(LoroError::FrontiersNotFound(i));
            }
        }

        let before = &oplog.dag.frontiers_to_vv(&state.frontiers).unwrap();
        let Some(after) = &oplog.dag.frontiers_to_vv(&frontiers) else {
            drop(oplog);
            drop(state);
            if had_txn {
                self.renew_txn_if_auto_commit(options);
            }
            return Err(LoroError::NotFoundError(
                format!("Cannot find the specified version {:?}", frontiers).into_boxed_str(),
            ));
        };

        self.set_detached(true);
        let (diff, diff_mode) =
            calc.calc_diff_internal(&oplog, before, &state.frontiers, after, &frontiers, None);
        state.apply_diff(
            InternalDocDiff {
                origin: "checkout".into(),
                diff: Cow::Owned(diff),
                by: EventTriggerKind::Checkout,
                new_version: Cow::Owned(frontiers.clone()),
            },
            diff_mode,
        );

        drop(state);
        drop(oplog);
        Ok(options)
    }

    #[inline]
    pub fn vv_to_frontiers(&self, vv: &VersionVector) -> Frontiers {
        self.oplog.try_lock().unwrap().dag.vv_to_frontiers(vv)
    }

    #[inline]
    pub fn frontiers_to_vv(&self, frontiers: &Frontiers) -> Option<VersionVector> {
        self.oplog
            .try_lock()
            .unwrap()
            .dag
            .frontiers_to_vv(frontiers)
    }

    /// Import ops from other doc.
    ///
    /// After `a.merge(b)` and `b.merge(a)`, `a` and `b` will have the same content if they are in attached mode.
    pub fn merge(&self, other: &Self) -> LoroResult<ImportStatus> {
        self.import(&other.export_from(&self.oplog_vv()))
    }

    pub(crate) fn arena(&self) -> &SharedArena {
        &self.arena
    }

    #[inline]
    pub fn len_ops(&self) -> usize {
        let oplog = self.oplog.try_lock().unwrap();
        let ans = oplog.vv().iter().map(|(_, ops)| *ops).sum::<i32>() as usize;
        if oplog.is_shallow() {
            let sub = oplog
                .shallow_since_vv()
                .iter()
                .map(|(_, ops)| *ops)
                .sum::<i32>() as usize;
            ans - sub
        } else {
            ans
        }
    }

    #[inline]
    pub fn len_changes(&self) -> usize {
        let oplog = self.oplog.try_lock().unwrap();
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
            static IS_CHECKING: AtomicBool = AtomicBool::new(false);
            if IS_CHECKING.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }

            IS_CHECKING.store(true, std::sync::atomic::Ordering::Release);
            let peer_id = self.peer_id();
            let s = info_span!("CheckStateDiffCalcConsistencySlow", ?peer_id);
            let _g = s.enter();
            let options = self.commit_then_stop();
            self.oplog.try_lock().unwrap().check_dag_correctness();
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
                let mut calculated_state = doc.app_state().try_lock().unwrap();
                let mut current_state = self.app_state().try_lock().unwrap();
                current_state.check_is_the_same(&mut calculated_state);
            } else {
                let f = self.state_frontiers();
                let vv = self
                    .oplog()
                    .try_lock()
                    .unwrap()
                    .dag
                    .frontiers_to_vv(&f)
                    .unwrap();
                let bytes = self.export(ExportMode::updates_till(&vv)).unwrap();
                let doc = Self::new();
                doc.import(&bytes).unwrap();
                let mut calculated_state = doc.app_state().try_lock().unwrap();
                let mut current_state = self.app_state().try_lock().unwrap();
                current_state.check_is_the_same(&mut calculated_state);
            }

            self.renew_txn_if_auto_commit(options);
            IS_CHECKING.store(false, std::sync::atomic::Ordering::Release);
        }
    }

    #[inline]
    pub fn log_estimated_size(&self) {
        let state = self.state.try_lock().unwrap();
        state.log_estimated_size();
    }

    pub fn query_pos(&self, pos: &Cursor) -> Result<PosQueryResult, CannotFindRelativePosition> {
        self.query_pos_internal(pos, true)
    }

    /// Get position in a seq container
    pub(crate) fn query_pos_internal(
        &self,
        pos: &Cursor,
        ret_event_index: bool,
    ) -> Result<PosQueryResult, CannotFindRelativePosition> {
        let mut state = self.state.try_lock().unwrap();
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

            // commit the txn to make sure we can query the history correctly
            drop(state);
            self.commit_then_renew();
            let oplog = self.oplog().try_lock().unwrap();
            // TODO: assert pos.id is not unknown
            if let Some(id) = pos.id {
                let idx = oplog
                    .arena
                    .id_to_idx(&pos.container)
                    .ok_or(CannotFindRelativePosition::ContainerDeleted)?;
                // We know where the target id is when we trace back to the delete_op_id.
                let Some(delete_op_id) = find_last_delete_op(&oplog, id, idx) else {
                    if oplog.shallow_since_vv().includes_id(id) {
                        return Err(CannotFindRelativePosition::HistoryCleared);
                    }

                    tracing::error!("Cannot find id {}", id);
                    return Err(CannotFindRelativePosition::IdNotFound);
                };
                // Should use persist mode so that it will force all the diff calculators to use the `checkout` mode
                let mut diff_calc = DiffCalculator::new(true);
                let before_frontiers: Frontiers = oplog.dag.find_deps_of_id(delete_op_id);
                let before = &oplog.dag.frontiers_to_vv(&before_frontiers).unwrap();
                // TODO: PERF: it doesn't need to calc the effects here
                diff_calc.calc_diff_internal(
                    &oplog,
                    before,
                    &before_frontiers,
                    oplog.vv(),
                    oplog.frontiers(),
                    Some(&|target| idx == target),
                );
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
        }
    }

    /// Free the history cache that is used for making checkout faster.
    ///
    /// If you use checkout that switching to an old/concurrent version, the history cache will be built.
    /// You can free it by calling this method.
    pub fn free_history_cache(&self) {
        self.oplog.try_lock().unwrap().free_history_cache();
    }

    /// Free the cached diff calculator that is used for checkout.
    pub fn free_diff_calculator(&self) {
        *self.diff_calculator.try_lock().unwrap() = DiffCalculator::new(true);
    }

    /// If you use checkout that switching to an old/concurrent version, the history cache will be built.
    /// You can free it by calling `free_history_cache`.
    pub fn has_history_cache(&self) -> bool {
        self.oplog.try_lock().unwrap().has_history_cache()
    }

    /// Encoded all ops and history cache to bytes and store them in the kv store.
    ///
    /// The parsed ops will be dropped
    #[inline]
    pub fn compact_change_store(&self) {
        self.commit_then_renew();
        self.oplog.try_lock().unwrap().compact_change_store();
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
        let mut state = self.state.try_lock().unwrap();
        let idx = state.arena.id_to_idx(id)?;
        state.get_path(idx)
    }

    #[instrument(skip(self))]
    pub fn export(&self, mode: ExportMode) -> Result<Vec<u8>, LoroEncodeError> {
        let options = self.commit_then_stop();
        let ans = match mode {
            ExportMode::Snapshot => export_fast_snapshot(self),
            ExportMode::Updates { from } => export_fast_updates(self, &from),
            ExportMode::UpdatesInRange { spans } => {
                export_fast_updates_in_range(&self.oplog.try_lock().unwrap(), spans.as_ref())
            }
            ExportMode::ShallowSnapshot(f) => export_shallow_snapshot(self, &f)?,
            ExportMode::StateOnly(f) => match f {
                Some(f) => export_state_only_snapshot(self, &f)?,
                None => export_state_only_snapshot(self, &self.oplog_frontiers())?,
            },
            ExportMode::SnapshotAt { version } => export_snapshot_at(self, &version)?,
        };

        self.renew_txn_if_auto_commit(options);
        Ok(ans)
    }

    /// The doc only contains the history since the shallow history start version vector.
    ///
    /// This is empty if the doc is not shallow.
    ///
    /// The ops included by the shallow history start version vector are not in the doc.
    pub fn shallow_since_vv(&self) -> ImVersionVector {
        self.oplog().try_lock().unwrap().shallow_since_vv().clone()
    }

    pub fn shallow_since_frontiers(&self) -> Frontiers {
        self.oplog()
            .try_lock()
            .unwrap()
            .shallow_since_frontiers()
            .clone()
    }

    /// Check if the doc contains the full history.
    pub fn is_shallow(&self) -> bool {
        !self
            .oplog()
            .try_lock()
            .unwrap()
            .shallow_since_vv()
            .is_empty()
    }

    /// Get the number of operations in the pending transaction.
    ///
    /// The pending transaction is the one that is not committed yet. It will be committed
    /// after calling `doc.commit()`, `doc.export(mode)` or `doc.checkout(version)`.
    pub fn get_pending_txn_len(&self) -> usize {
        if let Some(txn) = self.txn.try_lock().unwrap().as_ref() {
            txn.len()
        } else {
            0
        }
    }

    #[inline]
    pub fn find_id_spans_between(&self, from: &Frontiers, to: &Frontiers) -> VersionVectorDiff {
        self.oplog().try_lock().unwrap().dag.find_path(from, to)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ChangeTravelError {
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
        self.commit_then_renew();
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

        for id in ids {
            let op_log = &self.oplog().try_lock().unwrap();
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
            pending.push(PendingNode(ChangeMeta::from_change(
                &self.oplog().try_lock().unwrap().get_change_at(*id).unwrap(),
            )));
        }
        while let Some(PendingNode(node)) = pending.pop() {
            let deps = node.deps.clone();
            if f(node).is_break() {
                break;
            }

            for dep in deps.iter() {
                let Some(dep_node) = self.oplog().try_lock().unwrap().get_change_at(dep) else {
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
    }

    pub fn get_changed_containers_in(&self, id: ID, len: usize) -> FxHashSet<ContainerID> {
        self.commit_then_renew();
        let mut set = FxHashSet::default();
        let oplog = &self.oplog().try_lock().unwrap();
        for op in oplog.iter_ops(id.to_span(len)) {
            let id = oplog.arena.get_container_id(op.container()).unwrap();
            set.insert(id);
        }

        set
    }
}

// FIXME: PERF: This method is quite slow because it iterates all the changes
fn find_last_delete_op(oplog: &OpLog, id: ID, idx: ContainerIdx) -> Option<ID> {
    let start_vv = oplog
        .dag
        .frontiers_to_vv(&id.into())
        .unwrap_or_else(|| oplog.shallow_since_vv().to_vv());
    for change in oplog.iter_changes_causally_rev(&start_vv, oplog.vv()) {
        for op in change.ops.iter().rev() {
            if op.container != idx {
                continue;
            }
            if let InnerContent::List(InnerListOp::Delete(d)) = &op.content {
                if d.id_start.to_span(d.atom_len()).contains(id) {
                    return Some(ID::new(change.peer(), op.counter));
                }
            }
        }
    }

    None
}

#[derive(Debug)]
pub struct CommitWhenDrop<'a> {
    doc: &'a LoroDoc,
    default_options: CommitOptions,
}

impl Drop for CommitWhenDrop<'_> {
    fn drop(&mut self) {
        {
            let mut guard = self.doc.txn.try_lock().unwrap();
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
    use loro_common::ID;

    use crate::{version::Frontiers, LoroDoc, ToJson};

    #[test]
    fn test_sync() {
        fn is_send_sync<T: Send + Sync>(_v: T) {}
        let loro = super::LoroDoc::new();
        is_send_sync(loro)
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
            text.insert_with_txn(&mut txn, 0, &i.to_string()).unwrap();
            list.insert_with_txn(&mut txn, 0, i.into()).unwrap();
        }
        txn.commit().unwrap();
        let b = LoroDoc::new();
        b.import(&loro.export_snapshot().unwrap()).unwrap();
        loro.checkout(&Frontiers::default()).unwrap();
        {
            let json = &loro.get_deep_value();
            assert_eq!(json.to_json(), r#"{"text":"","list":[],"map":{}}"#);
        }

        b.checkout(&ID::new(1, 2).into()).unwrap();
        {
            let json = &b.get_deep_value();
            assert_eq!(json.to_json(), r#"{"text":"0","list":[0],"map":{"key":0}}"#);
        }

        loro.checkout(&ID::new(1, 3).into()).unwrap();
        {
            let json = &loro.get_deep_value();
            assert_eq!(json.to_json(), r#"{"text":"0","list":[0],"map":{"key":1}}"#);
        }

        b.checkout(&ID::new(1, 29).into()).unwrap();
        {
            let json = &b.get_deep_value();
            assert_eq!(
                json.to_json(),
                r#"{"text":"9876543210","list":[9,8,7,6,5,4,3,2,1,0],"map":{"key":9}}"#
            );
        }
    }

    #[test]
    fn import_batch_err_181() {
        let a = LoroDoc::new_auto_commit();
        let update_a = a.export_snapshot();
        let b = LoroDoc::new_auto_commit();
        b.import_batch(&[update_a.unwrap()]).unwrap();
        b.get_text("text").insert(0, "hello").unwrap();
        b.commit_then_renew();
        let oplog = b.oplog().try_lock().unwrap();
        drop(oplog);
        b.export_from(&Default::default());
    }
}
