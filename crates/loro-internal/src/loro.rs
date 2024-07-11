use std::{
    borrow::Cow,
    cmp::Ordering,
    sync::{
        atomic::{
            AtomicBool,
            Ordering::{Acquire, Release},
        },
        Arc, Mutex, Weak,
    },
};

use either::Either;
use fxhash::FxHashMap;
use itertools::Itertools;
use loro_common::{ContainerID, ContainerType, HasIdSpan, IdSpan, LoroResult, LoroValue, ID};
use rle::HasLength;
use tracing::{info_span, instrument};

use crate::{
    arena::SharedArena,
    change::Timestamp,
    configure::Configure,
    container::{
        idx::ContainerIdx, list::list_op::InnerListOp, richtext::config::StyleConfigMap,
        IntoContainerId,
    },
    cursor::{AbsolutePosition, CannotFindRelativePosition, Cursor, PosQueryResult},
    dag::DagUtils,
    encoding::{
        decode_snapshot, export_snapshot, json_schema::op::JsonSchema, parse_header_and_body,
        EncodeMode, ParsedHeaderAndBody,
    },
    event::{str_to_path, EventTriggerKind, Index},
    handler::{Handler, MovableListHandler, TextHandler, TreeHandler, ValueOrHandler},
    id::PeerID,
    op::InnerContent,
    oplog::dag::FrontiersNotIncluded,
    undo::DiffBatch,
    version::Frontiers,
    HandlerTrait, InternalString, LoroError, VersionVector,
};

use super::{
    diff_calc::DiffCalculator,
    event::InternalDocDiff,
    obs::{Observer, SubID, Subscriber},
    oplog::OpLog,
    state::DocState,
    txn::Transaction,
    ListHandler, MapHandler,
};

/// `LoroApp` serves as the library's primary entry point.
/// It's constituted by an [OpLog] and an [AppState].
///
/// - [OpLog] encompasses all operations, signifying the document history.
/// - [AppState] signifies the current document state.
///
/// They will share a [super::arena::SharedArena]
///
/// # Detached Mode
///
/// This mode enables separate usage of [OpLog] and [AppState].
/// It facilitates temporal navigation. [AppState] can be reverted to
/// any version contained within the [OpLog].
///
/// `LoroApp::detach()` separates [AppState] from [OpLog]. In this mode,
/// updates to [OpLog] won't affect [AppState], while updates to [AppState]
/// will continue to affect [OpLog].
pub struct LoroDoc {
    oplog: Arc<Mutex<OpLog>>,
    state: Arc<Mutex<DocState>>,
    arena: SharedArena,
    config: Configure,
    observer: Arc<Observer>,
    diff_calculator: Arc<Mutex<DiffCalculator>>,
    // when dropping the doc, the txn will be committed
    txn: Arc<Mutex<Option<Transaction>>>,
    auto_commit: AtomicBool,
    detached: AtomicBool,
}

impl Default for LoroDoc {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for LoroDoc {
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
        let global_txn = Arc::new(Mutex::new(None));
        let config: Configure = oplog.configure.clone();
        // share arena
        let state = DocState::new_arc(arena.clone(), Arc::downgrade(&global_txn), config.clone());
        Self {
            oplog: Arc::new(Mutex::new(oplog)),
            state,
            config,
            detached: AtomicBool::new(false),
            auto_commit: AtomicBool::new(false),
            observer: Arc::new(Observer::new(arena.clone())),
            diff_calculator: Arc::new(Mutex::new(DiffCalculator::new())),
            txn: global_txn,
            arena,
        }
    }

    pub fn fork(&self) -> Self {
        self.commit_then_stop();
        let arena = self.arena.fork();
        let config = self.config.fork();
        let txn = Arc::new(Mutex::new(None));
        let new_state =
            self.state
                .lock()
                .unwrap()
                .fork(arena.clone(), Arc::downgrade(&txn), config.clone());
        let doc = LoroDoc {
            oplog: Arc::new(Mutex::new(
                self.oplog()
                    .lock()
                    .unwrap()
                    .fork(arena.clone(), config.clone()),
            )),
            state: new_state,
            arena,
            config,
            observer: Arc::new(Observer::new(self.arena.clone())),
            diff_calculator: Arc::new(Mutex::new(DiffCalculator::new())),
            txn,
            auto_commit: AtomicBool::new(false),
            detached: AtomicBool::new(self.detached.load(std::sync::atomic::Ordering::Relaxed)),
        };

        if self.auto_commit.load(std::sync::atomic::Ordering::Relaxed) {
            doc.start_auto_commit();
        }

        self.renew_txn_if_auto_commit();
        doc
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

    /// Set the interval of mergeable changes, in milliseconds.
    ///
    /// If two continuous local changes are within the interval, they will be merged into one change.
    /// The default value is 1000 seconds.
    #[inline]
    pub fn set_change_merge_interval(&self, interval: i64) {
        self.config.set_merge_interval(interval);
    }

    /// Set the jitter of the tree position(Fractional Index).
    ///
    /// The jitter is used to avoid conflicts when multiple users are creating the node at the same position.
    /// value 0 is default, which means no jitter, any value larger than 0 will enable jitter.
    /// Generally speaking, jitter will affect the growth rate of document size.
    #[inline]
    pub fn set_fractional_index_jitter(&self, jitter: u8) {
        self.config.set_fractional_index_jitter(jitter);
    }

    #[inline]
    pub fn config_text_style(&self, text_style: StyleConfigMap) {
        *self.config.text_style_config.try_write().unwrap() = text_style;
    }

    /// Create a doc with auto commit enabled.
    #[inline]
    pub fn new_auto_commit() -> Self {
        let doc = Self::new();
        doc.start_auto_commit();
        doc
    }

    pub fn from_snapshot(bytes: &[u8]) -> LoroResult<Self> {
        let doc = Self::new();
        let ParsedHeaderAndBody { mode, body, .. } = parse_header_and_body(bytes)?;
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
        self.oplog.lock().unwrap().is_empty() && self.state.lock().unwrap().is_empty()
    }

    /// Whether [OpLog] and [DocState] are detached.
    #[inline(always)]
    pub fn is_detached(&self) -> bool {
        self.detached.load(Acquire)
    }

    #[allow(unused)]
    pub(super) fn from_existing(oplog: OpLog, state: DocState) -> Self {
        let obs = Observer::new(oplog.arena.clone());
        Self {
            arena: oplog.arena.clone(),
            observer: Arc::new(obs),
            config: Default::default(),
            auto_commit: AtomicBool::new(false),
            oplog: Arc::new(Mutex::new(oplog)),
            state: Arc::new(Mutex::new(state)),
            diff_calculator: Arc::new(Mutex::new(DiffCalculator::new())),
            txn: Arc::new(Mutex::new(None)),
            detached: AtomicBool::new(false),
        }
    }

    #[inline(always)]
    pub fn peer_id(&self) -> PeerID {
        self.state.lock().unwrap().peer
    }

    #[inline(always)]
    pub fn set_peer_id(&self, peer: PeerID) -> LoroResult<()> {
        if self.auto_commit.load(Acquire) {
            let mut doc_state = self.state.lock().unwrap();
            doc_state.peer = peer;
            drop(doc_state);

            let txn = self.txn.lock().unwrap().take();
            if let Some(txn) = txn {
                txn.commit().unwrap();
            }

            let new_txn = self.txn().unwrap();
            self.txn.lock().unwrap().replace(new_txn);
            return Ok(());
        }

        let mut doc_state = self.state.lock().unwrap();
        if doc_state.is_in_txn() {
            return Err(LoroError::TransactionError(
                "Cannot change peer id during transaction"
                    .to_string()
                    .into_boxed_str(),
            ));
        }

        doc_state.peer = peer;
        Ok(())
    }

    #[inline(always)]
    pub fn detach(&self) {
        self.detached.store(true, Release);
    }

    #[inline(always)]
    pub fn attach(&self) {
        self.checkout_to_latest()
    }

    /// Get the timestamp of the current state.
    /// It's the last edit time of the [DocState].
    pub fn state_timestamp(&self) -> Timestamp {
        let f = &self.state.lock().unwrap().frontiers;
        self.oplog.lock().unwrap().get_timestamp_of_version(f)
    }

    /// Create a new transaction.
    /// Every ops created inside one transaction will be packed into a single
    /// [Change].
    ///
    /// There can only be one active transaction at a time for a [LoroDoc].
    #[inline(always)]
    pub fn txn(&self) -> Result<Transaction, LoroError> {
        self.txn_with_origin("")
    }

    #[inline(always)]
    pub fn with_txn<F, R>(&self, f: F) -> LoroResult<R>
    where
        F: FnOnce(&mut Transaction) -> LoroResult<R>,
    {
        let mut txn = self.txn().unwrap();
        let v = f(&mut txn)?;
        txn.commit()?;
        Ok(v)
    }

    pub fn start_auto_commit(&self) {
        self.auto_commit.store(true, Release);
        let mut self_txn = self.txn.try_lock().unwrap();
        if self_txn.is_some() || self.detached.load(Acquire) {
            return;
        }

        let txn = self.txn().unwrap();
        self_txn.replace(txn);
    }

    /// Commit the cumulative auto commit transaction.
    /// This method only has effect when `auto_commit` is true.
    ///
    /// Afterwards, the users need to call `self.renew_txn_after_commit()` to resume the continuous transaction.
    #[inline]
    pub fn commit_then_stop(&self) {
        self.commit_with(CommitOptions::new().immediate_renew(false))
    }

    /// Commit the cumulative auto commit transaction.
    /// It will start the next one immediately
    #[inline]
    pub fn commit_then_renew(&self) {
        self.commit_with(CommitOptions::new().immediate_renew(true))
    }

    /// Commit the cumulative auto commit transaction.
    /// This method only has effect when `auto_commit` is true.
    /// If `immediate_renew` is true, a new transaction will be created after the old one is committed
    #[instrument(skip_all)]
    pub fn commit_with(&self, config: CommitOptions) {
        if !self.auto_commit.load(Acquire) {
            // if not auto_commit, nothing should happen
            // because the global txn is not used
            return;
        }

        let mut txn_guard = self.txn.try_lock().unwrap();
        let txn = txn_guard.take();
        drop(txn_guard);
        let Some(mut txn) = txn else {
            return;
        };

        let on_commit = txn.take_on_commit();
        if let Some(origin) = config.origin {
            txn.set_origin(origin);
        }

        if let Some(timestamp) = config.timestamp {
            txn.set_timestamp(timestamp);
        }

        txn.commit().unwrap();
        if config.immediate_renew {
            let mut txn_guard = self.txn.try_lock().unwrap();
            assert!(!self.detached.load(std::sync::atomic::Ordering::Acquire));
            *txn_guard = Some(self.txn().unwrap());
        }

        if let Some(on_commit) = on_commit {
            on_commit(&self.state);
        }
    }

    #[inline]
    pub fn renew_txn_if_auto_commit(&self) {
        if self.auto_commit.load(Acquire) && !self.detached.load(Acquire) {
            let mut self_txn = self.txn.try_lock().unwrap();
            if self_txn.is_some() {
                return;
            }

            let txn = self.txn().unwrap();
            self_txn.replace(txn);
        }
    }

    #[inline]
    pub(crate) fn get_global_txn(&self) -> Weak<Mutex<Option<Transaction>>> {
        Arc::downgrade(&self.txn)
    }

    /// Create a new transaction with specified origin.
    ///
    /// The origin will be propagated to the events.
    /// There can only be one active transaction at a time for a [LoroDoc].
    pub fn txn_with_origin(&self, origin: &str) -> Result<Transaction, LoroError> {
        if self.is_detached() {
            return Err(LoroError::TransactionError(
                String::from("LoroDoc is in detached mode. OpLog and AppState are using different version. So it's readonly.").into_boxed_str(),
            ));
        }

        let mut txn = Transaction::new_with_origin(
            self.state.clone(),
            self.oplog.clone(),
            origin.into(),
            self.get_global_txn(),
        );

        let obs = self.observer.clone();
        txn.set_on_commit(Box::new(move |state| {
            let mut state = state.try_lock().unwrap();
            let events = state.take_events();
            drop(state);
            for event in events {
                obs.emit(event);
            }
        }));

        Ok(txn)
    }

    #[inline(always)]
    pub fn app_state(&self) -> &Arc<Mutex<DocState>> {
        &self.state
    }

    #[inline]
    pub fn get_state_deep_value(&self) -> LoroValue {
        self.state.lock().unwrap().get_deep_value()
    }

    #[inline(always)]
    pub fn oplog(&self) -> &Arc<Mutex<OpLog>> {
        &self.oplog
    }

    pub fn export_from(&self, vv: &VersionVector) -> Vec<u8> {
        self.commit_then_stop();
        let ans = self.oplog.lock().unwrap().export_from(vv);
        self.renew_txn_if_auto_commit();
        ans
    }

    #[inline(always)]
    #[instrument(skip_all)]
    pub fn import(&self, bytes: &[u8]) -> Result<(), LoroError> {
        self.import_with(bytes, Default::default())
    }

    #[inline]
    pub fn import_with(&self, bytes: &[u8], origin: InternalString) -> Result<(), LoroError> {
        self.commit_then_stop();
        let ans = self._import_with(bytes, origin);
        self.renew_txn_if_auto_commit();
        ans
    }

    fn _import_with(&self, bytes: &[u8], origin: InternalString) -> Result<(), LoroError> {
        let parsed = parse_header_and_body(bytes)?;
        match parsed.mode.is_snapshot() {
            false => {
                if self.state.lock().unwrap().is_in_txn() {
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
                )?;
            }
            true => {
                if self.can_reset_with_snapshot() {
                    tracing::info!("Init by snapshot {}", self.peer_id());
                    decode_snapshot(self, parsed.mode, parsed.body)?;
                } else if parsed.mode == EncodeMode::Snapshot {
                    self.update_oplog_and_apply_delta_to_state_if_needed(
                        |oplog| oplog.decode(parsed),
                        origin,
                    )?;
                } else {
                    tracing::info!("Import from new doc");
                    let app = LoroDoc::new();
                    decode_snapshot(&app, parsed.mode, parsed.body)?;
                    let oplog = self.oplog.lock().unwrap();
                    // TODO: PERF: the ser and de can be optimized out
                    let updates = app.export_from(oplog.vv());
                    drop(oplog);

                    return self.import_with(&updates, origin);
                }
            }
        };

        self.emit_events();
        Ok(())
    }

    pub(crate) fn update_oplog_and_apply_delta_to_state_if_needed(
        &self,
        f: impl FnOnce(&mut OpLog) -> Result<(), LoroError>,
        origin: InternalString,
    ) -> Result<(), LoroError> {
        let mut oplog = self.oplog.lock().unwrap();
        let old_vv = oplog.vv().clone();
        let old_frontiers = oplog.frontiers().clone();
        f(&mut oplog)?;
        if !self.detached.load(Acquire) {
            let mut diff = DiffCalculator::default();
            let diff = diff.calc_diff_internal(
                &oplog,
                &old_vv,
                Some(&old_frontiers),
                oplog.vv(),
                Some(oplog.dag.get_frontiers()),
                None,
            );
            let mut state = self.state.lock().unwrap();
            state.apply_diff(InternalDocDiff {
                origin,
                diff: (diff).into(),
                by: EventTriggerKind::Import,
                new_version: Cow::Owned(oplog.frontiers().clone()),
            });
        } else {
            tracing::info!("Detached");
        }
        Ok(())
    }

    /// For fuzzing tests
    #[cfg(feature = "test_utils")]
    pub fn import_delta_updates_unchecked(&self, body: &[u8]) -> LoroResult<()> {
        self.commit_then_stop();
        let mut oplog = self.oplog.lock().unwrap();
        let old_vv = oplog.vv().clone();
        let old_frontiers = oplog.frontiers().clone();
        let ans = oplog.decode(ParsedHeaderAndBody {
            checksum: [0; 16],
            checksum_body: body,
            mode: EncodeMode::Rle,
            body,
        });
        if ans.is_ok() && !self.detached.load(Acquire) {
            let mut diff = DiffCalculator::default();
            let diff = diff.calc_diff_internal(
                &oplog,
                &old_vv,
                Some(&old_frontiers),
                oplog.vv(),
                Some(oplog.dag.get_frontiers()),
                None,
            );
            let mut state = self.state.lock().unwrap();
            state.apply_diff(InternalDocDiff {
                origin: "".into(),
                diff: (diff).into(),
                by: EventTriggerKind::Import,
                new_version: Cow::Owned(oplog.frontiers().clone()),
            });
        }
        self.renew_txn_if_auto_commit();
        ans
    }

    /// For fuzzing tests
    #[cfg(feature = "test_utils")]
    pub fn import_snapshot_unchecked(&self, bytes: &[u8]) -> LoroResult<()> {
        self.commit_then_stop();
        let ans = decode_snapshot(self, EncodeMode::Snapshot, bytes);
        self.renew_txn_if_auto_commit();
        ans
    }

    fn emit_events(&self) {
        // we should not hold the lock when emitting events
        let events = {
            let mut state = self.state.lock().unwrap();
            state.take_events()
        };
        for event in events {
            self.observer.emit(event);
        }
    }

    #[instrument(skip_all)]
    pub fn export_snapshot(&self) -> Vec<u8> {
        self.commit_then_stop();
        let ans = export_snapshot(self);
        self.renew_txn_if_auto_commit();
        ans
    }

    /// Import the json schema updates.
    ///
    /// only supports backward compatibility but not forward compatibility.
    pub fn import_json_updates<T: TryInto<JsonSchema>>(&self, json: T) -> LoroResult<()> {
        let json = json.try_into().map_err(|_| LoroError::InvalidJsonSchema)?;
        self.commit_then_stop();
        self.update_oplog_and_apply_delta_to_state_if_needed(
            |oplog| crate::encoding::json_schema::import_json(oplog, json),
            Default::default(),
        )?;
        self.emit_events();
        self.renew_txn_if_auto_commit();
        Ok(())
    }

    pub fn export_json_updates(
        &self,
        start_vv: &VersionVector,
        end_vv: &VersionVector,
    ) -> JsonSchema {
        self.commit_then_stop();
        let oplog = self.oplog.lock().unwrap();
        let json = crate::encoding::json_schema::export_json(&oplog, start_vv, end_vv);
        drop(oplog);
        self.renew_txn_if_auto_commit();
        json
    }

    /// Get the version vector of the current OpLog
    #[inline]
    pub fn oplog_vv(&self) -> VersionVector {
        self.oplog.lock().unwrap().vv().clone()
    }

    /// Get the version vector of the current [DocState]
    #[inline]
    pub fn state_vv(&self) -> VersionVector {
        let f = &self.state.lock().unwrap().frontiers;
        self.oplog.lock().unwrap().dag.frontiers_to_vv(f).unwrap()
    }

    pub fn get_by_path(&self, path: &[Index]) -> Option<ValueOrHandler> {
        let value: LoroValue = self.state.lock().unwrap().get_value_by_path(path)?;
        if let LoroValue::Container(c) = value {
            Some(ValueOrHandler::Handler(Handler::new_attached(
                c.clone(),
                self.arena.clone(),
                self.get_global_txn(),
                Arc::downgrade(&self.state),
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

    #[inline]
    pub fn get_handler(&self, id: ContainerID) -> Handler {
        Handler::new_attached(
            id,
            self.arena.clone(),
            self.get_global_txn(),
            Arc::downgrade(&self.state),
        )
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_text<I: IntoContainerId>(&self, id: I) -> TextHandler {
        let id = id.into_container_id(&self.arena, ContainerType::Text);
        Handler::new_attached(
            id,
            self.arena.clone(),
            self.get_global_txn(),
            Arc::downgrade(&self.state),
        )
        .into_text()
        .unwrap()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_list<I: IntoContainerId>(&self, id: I) -> ListHandler {
        let id = id.into_container_id(&self.arena, ContainerType::List);
        Handler::new_attached(
            id,
            self.arena.clone(),
            self.get_global_txn(),
            Arc::downgrade(&self.state),
        )
        .into_list()
        .unwrap()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_movable_list<I: IntoContainerId>(&self, id: I) -> MovableListHandler {
        let id = id.into_container_id(&self.arena, ContainerType::MovableList);
        Handler::new_attached(
            id,
            self.arena.clone(),
            self.get_global_txn(),
            Arc::downgrade(&self.state),
        )
        .into_movable_list()
        .unwrap()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_map<I: IntoContainerId>(&self, id: I) -> MapHandler {
        let id = id.into_container_id(&self.arena, ContainerType::Map);
        Handler::new_attached(
            id,
            self.arena.clone(),
            self.get_global_txn(),
            Arc::downgrade(&self.state),
        )
        .into_map()
        .unwrap()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_tree<I: IntoContainerId>(&self, id: I) -> TreeHandler {
        let id = id.into_container_id(&self.arena, ContainerType::Tree);
        Handler::new_attached(
            id,
            self.arena.clone(),
            self.get_global_txn(),
            Arc::downgrade(&self.state),
        )
        .into_tree()
        .unwrap()
    }

    #[cfg(feature = "counter")]
    pub fn get_counter<I: IntoContainerId>(
        &self,
        id: I,
    ) -> crate::handler::counter::CounterHandler {
        let id = id.into_container_id(&self.arena, ContainerType::Counter);
        Handler::new_attached(
            id,
            self.arena.clone(),
            self.get_global_txn(),
            Arc::downgrade(&self.state),
        )
        .into_counter()
        .unwrap()
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
        if self.is_detached() {
            return Err(LoroError::EditWhenDetached);
        }

        self.commit_then_stop();
        if !self
            .oplog()
            .lock()
            .unwrap()
            .vv()
            .includes_id(id_span.id_last())
        {
            self.renew_txn_if_auto_commit();
            return Err(LoroError::UndoInvalidIdSpan(id_span.id_last()));
        }

        let (was_recording, latest_frontiers) = {
            let mut state = self.state.lock().unwrap();
            let was_recording = state.is_recording();
            state.stop_and_clear_recording();
            (was_recording, state.frontiers.clone())
        };

        let spans = self.oplog.lock().unwrap().split_span_based_on_deps(id_span);
        let diff = crate::undo::undo(
            spans,
            match post_transform_base {
                Some(d) => Either::Right(d),
                None => Either::Left(&latest_frontiers),
            },
            |from, to| {
                self.checkout_without_emitting(from).unwrap();
                self.state.lock().unwrap().start_recording();
                self.checkout_without_emitting(to).unwrap();
                let mut state = self.state.lock().unwrap();
                let e = state.take_events();
                state.stop_and_clear_recording();
                DiffBatch::new(e)
            },
            before_diff,
        );

        // println!("\nundo_internal: diff: {:?}", diff);

        self.checkout_without_emitting(&latest_frontiers)?;
        self.detached.store(false, Release);
        if was_recording {
            self.state.lock().unwrap().start_recording();
        }
        self.start_auto_commit();
        self.apply_diff(diff, container_remap, true).unwrap();
        Ok(CommitWhenDrop {
            doc: self,
            options: CommitOptions::new().origin("undo"),
        })
    }

    /// Calculate the diff between the current state and the target state, and apply the diff to the current state.
    pub fn diff_and_apply(&self, target: &Frontiers) -> LoroResult<()> {
        let f = self.state_frontiers();
        let diff = self.diff(&f, target)?;
        self.apply_diff(diff, &mut Default::default(), false)
    }

    /// Calculate the diff between two versions so that apply diff on a will make the state same as b.
    ///
    /// NOTE: This method will make the doc enter the **detached mode**.
    pub fn diff(&self, a: &Frontiers, b: &Frontiers) -> LoroResult<DiffBatch> {
        {
            // check whether a and b are valid
            let oplog = self.oplog.lock().unwrap();
            for &id in a.iter() {
                if !oplog.dag.contains(id) {
                    return Err(LoroError::FrontiersNotFound(id));
                }
            }
            for &id in b.iter() {
                if !oplog.dag.contains(id) {
                    return Err(LoroError::FrontiersNotFound(id));
                }
            }
        }

        self.commit_then_stop();

        let ans = {
            self.state.lock().unwrap().stop_and_clear_recording();
            self.checkout_without_emitting(a).unwrap();
            self.state.lock().unwrap().start_recording();
            self.checkout_without_emitting(b).unwrap();
            let mut state = self.state.lock().unwrap();
            let e = state.take_events();
            state.stop_and_clear_recording();
            DiffBatch::new(e)
        };

        Ok(ans)
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
    pub fn apply_diff(
        &self,
        mut diff: DiffBatch,
        container_remap: &mut FxHashMap<ContainerID, ContainerID>,
        skip_unreachable: bool,
    ) -> LoroResult<()> {
        if self.is_detached() {
            return Err(LoroError::EditWhenDetached);
        }

        // Sort container from the top to the bottom, so that we can have correct container remap
        let containers = diff.0.keys().cloned().sorted_by_cached_key(|cid| {
            let idx = self.arena.id_to_idx(cid).unwrap();
            self.arena.get_depth(idx).unwrap().get()
        });

        for mut id in containers {
            let mut remapped = false;
            let diff = diff.0.remove(&id).unwrap();

            while let Some(rid) = container_remap.get(&id) {
                remapped = true;
                id = rid.clone();
            }

            if skip_unreachable && !remapped && !self.state.lock().unwrap().get_reachable(&id) {
                continue;
            }

            let h = self.get_handler(id);
            h.apply_diff(diff, container_remap).unwrap();
        }

        Ok(())
    }

    /// This is for debugging purpose. It will travel the whole oplog
    #[inline]
    pub fn diagnose_size(&self) {
        self.oplog().lock().unwrap().diagnose_size();
    }

    #[inline]
    pub fn oplog_frontiers(&self) -> Frontiers {
        self.oplog().lock().unwrap().frontiers().clone()
    }

    #[inline]
    pub fn state_frontiers(&self) -> Frontiers {
        self.state.lock().unwrap().frontiers.clone()
    }

    /// - Ordering::Less means self is less than target or parallel
    /// - Ordering::Equal means versions equal
    /// - Ordering::Greater means self's version is greater than target
    #[inline]
    pub fn cmp_with_frontiers(&self, other: &Frontiers) -> Ordering {
        self.oplog().lock().unwrap().cmp_with_frontiers(other)
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
        self.oplog().lock().unwrap().cmp_frontiers(a, b)
    }

    pub fn subscribe_root(&self, callback: Subscriber) -> SubID {
        let mut state = self.state.lock().unwrap();
        if !state.is_recording() {
            state.start_recording();
        }

        self.observer.subscribe_root(callback)
    }

    pub fn subscribe(&self, container_id: &ContainerID, callback: Subscriber) -> SubID {
        let mut state = self.state.lock().unwrap();
        if !state.is_recording() {
            state.start_recording();
        }

        self.observer.subscribe(container_id, callback)
    }

    #[inline]
    pub fn unsubscribe(&self, id: SubID) {
        self.observer.unsubscribe(id);
    }

    // PERF: opt
    pub fn import_batch(&self, bytes: &[Vec<u8>]) -> LoroResult<()> {
        self.commit_then_stop();
        let is_detached = self.is_detached();
        self.detach();
        self.oplog.lock().unwrap().batch_importing = true;
        let mut err = None;
        for data in bytes.iter() {
            match self.import(data) {
                Ok(_) => {}
                Err(e) => {
                    err = Some(e);
                }
            }
        }

        let mut oplog = self.oplog.lock().unwrap();
        oplog.batch_importing = false;
        oplog.dag.refresh_frontiers();
        drop(oplog);

        if !is_detached {
            self.checkout_to_latest();
        }

        self.renew_txn_if_auto_commit();
        if let Some(err) = err {
            return Err(err);
        }

        Ok(())
    }

    /// Get deep value of the document.
    #[inline]
    pub fn get_deep_value(&self) -> LoroValue {
        self.state.lock().unwrap().get_deep_value()
    }

    /// Get deep value of the document with container id
    #[inline]
    pub fn get_deep_value_with_id(&self) -> LoroValue {
        self.state.lock().unwrap().get_deep_value_with_id()
    }

    pub fn checkout_to_latest(&self) {
        if !self.is_detached() {
            self.commit_then_renew();
            return;
        }

        tracing::info_span!("CheckoutToLatest", peer = self.peer_id()).in_scope(|| {
            let f = self.oplog_frontiers();
            self.checkout(&f).unwrap();
            self.detached.store(false, Release);
            self.renew_txn_if_auto_commit();
        });
    }

    /// Checkout [DocState] to a specific version.
    ///
    /// This will make the current [DocState] detached from the latest version of [OpLog].
    /// Any further import will not be reflected on the [DocState], until user call [LoroDoc::attach()]
    pub fn checkout(&self, frontiers: &Frontiers) -> LoroResult<()> {
        self.checkout_without_emitting(frontiers)?;
        self.emit_events();
        Ok(())
    }

    #[instrument(level = "info", skip(self))]
    fn checkout_without_emitting(&self, frontiers: &Frontiers) -> Result<(), LoroError> {
        self.commit_then_stop();
        let oplog = self.oplog.lock().unwrap();
        let mut state = self.state.lock().unwrap();
        self.detached.store(true, Release);
        let mut calc = self.diff_calculator.lock().unwrap();
        for &i in frontiers.iter() {
            if !oplog.dag.contains(i) {
                return Err(LoroError::FrontiersNotFound(i));
            }
        }

        let before = &oplog.dag.frontiers_to_vv(&state.frontiers).unwrap();
        let Some(after) = &oplog.dag.frontiers_to_vv(frontiers) else {
            return Err(LoroError::NotFoundError(
                format!("Cannot find the specified version {:?}", frontiers).into_boxed_str(),
            ));
        };
        let diff = calc.calc_diff_internal(
            &oplog,
            before,
            Some(&state.frontiers),
            after,
            Some(frontiers),
            None,
        );
        state.apply_diff(InternalDocDiff {
            origin: "checkout".into(),
            diff: Cow::Owned(diff),
            by: EventTriggerKind::Checkout,
            new_version: Cow::Owned(frontiers.clone()),
        });
        Ok(())
    }

    #[inline]
    pub fn vv_to_frontiers(&self, vv: &VersionVector) -> Frontiers {
        self.oplog.lock().unwrap().dag.vv_to_frontiers(vv)
    }

    #[inline]
    pub fn frontiers_to_vv(&self, frontiers: &Frontiers) -> Option<VersionVector> {
        self.oplog.lock().unwrap().dag.frontiers_to_vv(frontiers)
    }

    /// Import ops from other doc.
    ///
    /// After `a.merge(b)` and `b.merge(a)`, `a` and `b` will have the same content if they are in attached mode.
    pub fn merge(&self, other: &Self) -> LoroResult<()> {
        self.import(&other.export_from(&self.oplog_vv()))
    }

    #[cfg(feature = "test_utils")]
    #[allow(unused)]
    pub(crate) fn arena(&self) -> &SharedArena {
        &self.arena
    }

    #[inline]
    pub fn len_ops(&self) -> usize {
        let oplog = self.oplog.lock().unwrap();
        oplog.vv().iter().map(|(_, ops)| *ops).sum::<i32>() as usize
    }

    #[inline]
    pub fn len_changes(&self) -> usize {
        let oplog = self.oplog.lock().unwrap();
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
        #[cfg(any(test, debug_assertions))]
        {
            static IS_CHECKING: AtomicBool = AtomicBool::new(false);
            if IS_CHECKING.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }

            IS_CHECKING.store(true, std::sync::atomic::Ordering::Release);
            let peer_id = self.peer_id();
            let s = info_span!("CheckStateDiffCalcConsistencySlow", ?peer_id);
            let _g = s.enter();
            self.commit_then_stop();
            let bytes = self.export_from(&Default::default());
            let doc = Self::new();
            doc.detach();
            doc.import(&bytes).unwrap();
            doc.checkout(&self.state_frontiers()).unwrap();
            let mut calculated_state = doc.app_state().try_lock().unwrap();
            let mut current_state = self.app_state().try_lock().unwrap();
            current_state.check_is_the_same(&mut calculated_state);
            self.renew_txn_if_auto_commit();
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
        let mut state = self.state.lock().unwrap();
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
            let oplog = self.oplog().lock().unwrap();
            // TODO: assert pos.id is not unknown
            if let Some(id) = pos.id {
                let idx = oplog
                    .arena
                    .id_to_idx(&pos.container)
                    .ok_or(CannotFindRelativePosition::ContainerDeleted)?;
                // We know where the target id is when we trace back to the delete_op_id.
                let delete_op_id = find_last_delete_op(&oplog, id, idx).unwrap();
                let mut diff_calc = DiffCalculator::new();
                let before_frontiers: Frontiers = oplog.dag.find_deps_of_id(delete_op_id);
                let before = &oplog.dag.frontiers_to_vv(&before_frontiers).unwrap();
                // TODO: PERF: it doesn't need to calc the effects here
                diff_calc.calc_diff_internal(
                    &oplog,
                    before,
                    Some(&before_frontiers),
                    &oplog.dag.vv,
                    Some(&oplog.dag.frontiers),
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
}

fn find_last_delete_op(oplog: &OpLog, id: ID, idx: ContainerIdx) -> Option<ID> {
    let start_vv = oplog.dag.frontiers_to_vv(&id.into()).unwrap();
    for change in oplog.iter_changes_causally_rev(&start_vv, &oplog.dag.vv) {
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
    options: CommitOptions,
}

impl<'a> Drop for CommitWhenDrop<'a> {
    fn drop(&mut self) {
        self.doc.commit_with(std::mem::take(&mut self.options));
    }
}

#[derive(Debug, Clone)]
pub struct CommitOptions {
    origin: Option<InternalString>,
    immediate_renew: bool,
    timestamp: Option<Timestamp>,
    commit_msg: Option<Box<str>>,
}

impl CommitOptions {
    pub fn new() -> Self {
        Self {
            origin: None,
            immediate_renew: true,
            timestamp: None,
            commit_msg: None,
        }
    }

    pub fn origin(mut self, origin: &str) -> Self {
        self.origin = Some(origin.into());
        self
    }

    pub fn immediate_renew(mut self, immediate_renew: bool) -> Self {
        self.immediate_renew = immediate_renew;
        self
    }

    pub fn timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    pub fn commit_msg(mut self, commit_msg: &str) -> Self {
        self.commit_msg = Some(commit_msg.into());
        self
    }

    pub fn set_origin(&mut self, origin: Option<&str>) {
        self.origin = origin.map(|x| x.into())
    }

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
        b.import(&loro.export_snapshot()).unwrap();
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
        b.import_batch(&[update_a]).unwrap();
        b.get_text("text").insert(0, "hello").unwrap();
        b.commit_then_renew();
        let oplog = b.oplog().lock().unwrap();
        dbg!(&oplog.arena);
        drop(oplog);
        b.export_from(&Default::default());
    }
}
