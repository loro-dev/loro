use std::{
    borrow::Cow,
    cmp::Ordering,
    sync::{Arc, Mutex, Weak},
};

use loro_common::{ContainerID, ContainerType, LoroResult, LoroValue};

use crate::{
    arena::SharedArena,
    change::Timestamp,
    container::{idx::ContainerIdx, IntoContainerId},
    encoding::{EncodeMode, ENCODE_SCHEMA_VERSION, MAGIC_BYTES},
    handler::TextHandler,
    handler::TreeHandler,
    id::PeerID,
    version::Frontiers,
    InternalString, LoroError, VersionVector,
};

use super::{
    diff_calc::DiffCalculator,
    encoding::encode_snapshot::{decode_app_snapshot, encode_app_snapshot},
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
    observer: Arc<Observer>,
    diff_calculator: Arc<Mutex<DiffCalculator>>,
    // when dropping the doc, the txn will be commited
    txn: Arc<Mutex<Option<Transaction>>>,
    auto_commit: bool,
    detached: bool,
}

impl Default for LoroDoc {
    fn default() -> Self {
        Self::new()
    }
}

impl LoroDoc {
    pub fn new() -> Self {
        let oplog = OpLog::new();
        let arena = oplog.arena.clone();
        // share arena
        let state = Arc::new(Mutex::new(DocState::new(arena.clone())));
        Self {
            oplog: Arc::new(Mutex::new(oplog)),
            state,
            detached: false,
            auto_commit: false,
            observer: Arc::new(Observer::new(arena.clone())),
            diff_calculator: Arc::new(Mutex::new(DiffCalculator::new())),
            txn: Arc::new(Mutex::new(None)),
            arena,
        }
    }

    /// Create a doc with auto commit enabled.
    #[inline]
    pub fn new_auto_commit() -> Self {
        let mut doc = Self::new();
        doc.start_auto_commit();
        doc
    }

    pub fn from_snapshot(bytes: &[u8]) -> LoroResult<Self> {
        let doc = Self::new();
        let (input, mode) = parse_encode_header(bytes)?;
        match mode {
            EncodeMode::Snapshot => {
                decode_app_snapshot(&doc, input, true)?;
                Ok(doc)
            }
            _ => Err(LoroError::DecodeError(
                "Invalid encode mode".to_string().into(),
            )),
        }
    }

    /// Is the document empty? (no ops)
    #[inline(always)]
    pub fn can_reset_with_snapshot(&self) -> bool {
        self.oplog.lock().unwrap().is_empty() && self.state.lock().unwrap().is_empty()
    }

    /// Whether [OpLog] ans [DocState] are detached.
    #[inline(always)]
    pub fn is_detached(&self) -> bool {
        self.detached
    }

    #[allow(unused)]
    pub(super) fn from_existing(oplog: OpLog, state: DocState) -> Self {
        let obs = Observer::new(oplog.arena.clone());
        Self {
            arena: oplog.arena.clone(),
            observer: Arc::new(obs),
            auto_commit: false,
            oplog: Arc::new(Mutex::new(oplog)),
            state: Arc::new(Mutex::new(state)),
            diff_calculator: Arc::new(Mutex::new(DiffCalculator::new())),
            txn: Arc::new(Mutex::new(None)),
            detached: false,
        }
    }

    #[inline(always)]
    pub fn peer_id(&self) -> PeerID {
        self.state.lock().unwrap().peer
    }

    #[inline(always)]
    pub fn set_peer_id(&self, peer: PeerID) -> LoroResult<()> {
        if self.auto_commit {
            let mut doc_state = self.state.lock().unwrap();
            doc_state.peer = peer;
            drop(doc_state);

            let txn = self.txn.lock().unwrap().take();
            if let Some(txn) = txn {
                if !txn.is_empty() {
                    txn.commit().unwrap();
                } else {
                    txn.abort();
                }
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
    pub fn detach(&mut self) {
        self.detached = true;
    }

    #[inline(always)]
    pub fn attach(&mut self) {
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

    pub fn start_auto_commit(&mut self) {
        self.auto_commit = true;
        let mut self_txn = self.txn.try_lock().unwrap();
        if self_txn.is_some() || self.detached {
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
        self.commit_with(None, None, false)
    }

    /// Commit the cumulative auto commit transaction.
    /// It will start the next one immediately
    #[inline]
    pub fn commit_then_renew(&self) {
        self.commit_with(None, None, true)
    }

    /// Commit the cumulative auto commit transaction.
    /// This method only has effect when `auto_commit` is true.
    /// If `immediate_renew` is true, a new transaction will be created after the old one is commited
    pub fn commit_with(
        &self,
        origin: Option<InternalString>,
        timestamp: Option<Timestamp>,
        immediate_renew: bool,
    ) {
        if !self.auto_commit {
            return;
        }

        let mut txn_guard = self.txn.try_lock().unwrap();
        let txn = txn_guard.take();
        drop(txn_guard);
        let Some(mut txn) = txn else {
            return;
        };

        let on_commit = txn.take_on_commit();
        if let Some(origin) = origin {
            txn.set_origin(origin);
        }

        if let Some(timestamp) = timestamp {
            txn.set_timestamp(timestamp);
        }

        txn.commit().unwrap();
        if immediate_renew {
            let mut txn_guard = self.txn.try_lock().unwrap();
            assert!(!self.detached);
            *txn_guard = Some(self.txn().unwrap());
        }

        if let Some(on_commit) = on_commit {
            on_commit(&self.state);
        }
    }

    /// Abort the current auto commit transaction.
    ///
    /// Afterwards, the users need to call `self.renew_txn_after_commit()` to resume the continuous transaction.
    #[inline]
    pub fn abort_txn(&self) {
        if let Some(mut txn) = self.txn.lock().unwrap().take() {
            txn.take_on_commit();
            txn.abort();
        }
    }

    pub fn renew_txn_if_auto_commit(&self) {
        if self.auto_commit && !self.detached {
            let mut self_txn = self.txn.try_lock().unwrap();
            if self_txn.is_some() {
                return;
            }

            let txn = self.txn().unwrap();
            self_txn.replace(txn);
        }
    }

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
    pub fn import(&self, bytes: &[u8]) -> Result<(), LoroError> {
        self.import_with(bytes, Default::default())
    }

    #[inline]
    pub fn import_without_state(&mut self, bytes: &[u8]) -> Result<(), LoroError> {
        self.commit_then_stop();
        self.detach();
        self.import(bytes)
    }

    #[inline]
    pub fn import_with(&self, bytes: &[u8], origin: InternalString) -> Result<(), LoroError> {
        self.commit_then_stop();
        let ans = self._import_with(bytes, origin);
        self.renew_txn_if_auto_commit();
        ans
    }

    fn _import_with(
        &self,
        bytes: &[u8],
        origin: string_cache::Atom<string_cache::EmptyStaticAtomSet>,
    ) -> Result<(), LoroError> {
        let (input, mode) = parse_encode_header(bytes)?;
        match mode {
            EncodeMode::Updates | EncodeMode::RleUpdates | EncodeMode::CompressedRleUpdates => {
                // TODO: need to throw error if state is in transaction
                debug_log::group!("import to {}", self.peer_id());
                let mut oplog = self.oplog.lock().unwrap();
                let old_vv = oplog.vv().clone();
                let old_frontiers = oplog.frontiers().clone();
                oplog.decode(bytes)?;
                if !self.detached {
                    let mut diff = DiffCalculator::default();
                    let diff = diff.calc_diff_internal(
                        &oplog,
                        &old_vv,
                        Some(&old_frontiers),
                        oplog.vv(),
                        Some(oplog.dag.get_frontiers()),
                    );
                    let mut state = self.state.lock().unwrap();
                    state.apply_diff(InternalDocDiff {
                        origin,
                        local: false,
                        diff: (diff).into(),
                        new_version: Cow::Owned(oplog.frontiers().clone()),
                    });
                }

                debug_log::group_end!();
            }
            EncodeMode::Snapshot => {
                if self.can_reset_with_snapshot() {
                    decode_app_snapshot(self, input, !self.detached)?;
                } else {
                    let app = LoroDoc::new();
                    decode_app_snapshot(&app, input, false)?;
                    let oplog = self.oplog.lock().unwrap();
                    // TODO: PERF: the ser and de can be optimized out
                    let updates = app.export_from(oplog.vv());
                    drop(oplog);
                    return self.import_with(&updates, origin);
                }
            }
            EncodeMode::Auto => unreachable!(),
        };
        self.emit_events();
        Ok(())
    }

    fn emit_events(&self) {
        let events = self.state.lock().unwrap().take_events();
        for event in events {
            self.observer.emit(event);
        }
    }

    pub fn export_snapshot(&self) -> Vec<u8> {
        self.commit_then_stop();
        debug_log::group!("export snapshot");
        let version = ENCODE_SCHEMA_VERSION;
        let mut ans = Vec::from(MAGIC_BYTES);
        // maybe u8 is enough
        ans.push(version);
        ans.push((EncodeMode::Snapshot).to_byte());
        ans.extend(encode_app_snapshot(self));
        debug_log::group_end!();
        self.renew_txn_if_auto_commit();
        ans
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

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_text<I: IntoContainerId>(&self, id: I) -> TextHandler {
        let idx = self.get_container_idx(id, ContainerType::Text);
        TextHandler::new(self.get_global_txn(), idx, Arc::downgrade(&self.state))
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_list<I: IntoContainerId>(&self, id: I) -> ListHandler {
        let idx = self.get_container_idx(id, ContainerType::List);
        ListHandler::new(self.get_global_txn(), idx, Arc::downgrade(&self.state))
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_map<I: IntoContainerId>(&self, id: I) -> MapHandler {
        let idx = self.get_container_idx(id, ContainerType::Map);
        MapHandler::new(self.get_global_txn(), idx, Arc::downgrade(&self.state))
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    #[inline]
    pub fn get_tree<I: IntoContainerId>(&self, id: I) -> TreeHandler {
        let idx = self.get_container_idx(id, ContainerType::Tree);
        TreeHandler::new(self.get_global_txn(), idx, Arc::downgrade(&self.state))
    }

    /// This is for debugging purpose. It will travel the whole oplog
    #[inline]
    pub fn diagnose_size(&self) {
        self.oplog().lock().unwrap().diagnose_size();
    }

    #[inline]
    fn get_container_idx<I: IntoContainerId>(&self, id: I, c_type: ContainerType) -> ContainerIdx {
        let id = id.into_container_id(&self.arena, c_type);
        self.arena.register_container(&id)
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
    pub fn cmp_frontiers(&self, other: &Frontiers) -> Ordering {
        self.oplog().lock().unwrap().cmp_frontiers(other)
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
    pub fn import_batch(&mut self, bytes: &[Vec<u8>]) -> LoroResult<()> {
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

    pub fn checkout_to_latest(&mut self) {
        let f = self.oplog_frontiers();
        self.checkout(&f).unwrap();
        self.detached = false;
        self.renew_txn_if_auto_commit();
    }

    /// Checkout [DocState] to a specific version.
    ///
    /// This will make the current [DocState] detached from the latest version of [OpLog].
    /// Any further import will not be reflected on the [DocState], until user call [LoroDoc::attach()]
    pub fn checkout(&mut self, frontiers: &Frontiers) -> LoroResult<()> {
        self.commit_then_stop();
        let oplog = self.oplog.lock().unwrap();
        let mut state = self.state.lock().unwrap();
        self.detached = true;
        let mut calc = self.diff_calculator.lock().unwrap();
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
        );
        state.apply_diff(InternalDocDiff {
            origin: "checkout".into(),
            local: true,
            diff: Cow::Owned(diff),
            new_version: Cow::Owned(frontiers.clone()),
        });
        let events = state.take_events();
        for event in events {
            self.observer.emit(event);
        }
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
}

fn parse_encode_header(bytes: &[u8]) -> Result<(&[u8], EncodeMode), LoroError> {
    if bytes.len() <= 6 {
        return Err(LoroError::DecodeError("Invalid import data".into()));
    }
    let (magic_bytes, input) = bytes.split_at(4);
    let magic_bytes: [u8; 4] = magic_bytes.try_into().unwrap();
    if magic_bytes != MAGIC_BYTES {
        return Err(LoroError::DecodeError("Invalid header bytes".into()));
    }
    let (version, input) = input.split_at(1);
    if version != [ENCODE_SCHEMA_VERSION] {
        return Err(LoroError::DecodeError("Invalid version".into()));
    }
    let mode: EncodeMode = input[0].try_into()?;
    Ok((&input[1..], mode))
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
        let mut loro = LoroDoc::new();
        loro.set_peer_id(1).unwrap();
        let text = loro.get_text("text");
        let map = loro.get_map("map");
        let list = loro.get_list("list");
        let mut txn = loro.txn().unwrap();
        for i in 0..10 {
            map.insert(&mut txn, "key", i.into()).unwrap();
            text.insert(&mut txn, 0, &i.to_string()).unwrap();
            list.insert(&mut txn, 0, i.into()).unwrap();
        }
        txn.commit().unwrap();
        let mut b = LoroDoc::new();
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
}
