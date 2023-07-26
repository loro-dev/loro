use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

use debug_log::debug_dbg;
use loro_common::{ContainerID, ContainerType, LoroValue};

use crate::{
    container::{registry::ContainerIdx, ContainerIdRaw},
    id::PeerID,
    log_store::encoding::{ConcreteEncodeMode, ENCODE_SCHEMA_VERSION, MAGIC_BYTES},
    EncodeMode, InternalString, LoroError, VersionVector,
};

use super::{
    diff_calc::DiffCalculator,
    event::InternalDocDiff,
    obs::{Observer, SubID, Subscriber},
    oplog::OpLog,
    snapshot_encode::{decode_app_snapshot, encode_app_snapshot},
    state::DocState,
    txn::Transaction,
    ListHandler, MapHandler, TextHandler,
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
    observer: Arc<Observer>,
    detached: bool,
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
            observer: Arc::new(Observer::new(arena)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.oplog.lock().unwrap().is_empty() && self.state.lock().unwrap().is_empty()
    }

    pub(super) fn from_existing(oplog: OpLog, state: DocState) -> Self {
        let obs = Observer::new(oplog.arena.clone());
        Self {
            observer: Arc::new(obs),
            oplog: Arc::new(Mutex::new(oplog)),
            state: Arc::new(Mutex::new(state)),
            detached: false,
        }
    }

    pub fn set_peer_id(&self, peer: PeerID) {
        self.state.lock().unwrap().peer = peer;
    }

    pub fn detach(&mut self) {
        self.detached = true;
    }

    pub fn attach(&mut self) {
        self.detached = false;
        let mut state = self.state.lock().unwrap();
        let oplog = self.oplog.lock().unwrap();
        let state_vv = oplog.dag.frontiers_to_vv(&state.frontiers);
        let mut diff = DiffCalculator::new();
        let diff = diff.calc_diff_internal(
            &oplog,
            &state_vv,
            Some(&state.frontiers),
            oplog.vv(),
            Some(oplog.frontiers()),
        );
        state.apply_diff(InternalDocDiff {
            local: true,
            origin: Default::default(),
            diff: (&diff).into(),
            new_version: Cow::Borrowed(oplog.frontiers()),
        });
    }

    pub fn txn(&self) -> Result<Transaction, LoroError> {
        if self.state.lock().unwrap().is_in_txn() {
            return Err(LoroError::DuplicatedTransactionError);
        }
        let mut txn = Transaction::new(self.state.clone(), self.oplog.clone());
        let obs = self.observer.clone();
        txn.set_on_commit(Box::new(move |state| {
            let events = state.lock().unwrap().take_events();
            for event in events {
                obs.emit(event);
            }
        }));

        Ok(txn)
    }

    pub fn app_state(&self) -> &Arc<Mutex<DocState>> {
        &self.state
    }

    pub fn get_state_deep_value(&self) -> LoroValue {
        self.state.lock().unwrap().get_deep_value()
    }

    pub fn oplog(&self) -> &Arc<Mutex<OpLog>> {
        &self.oplog
    }

    pub fn export_from(&self, vv: &VersionVector) -> Vec<u8> {
        self.oplog.lock().unwrap().export_from(vv)
    }

    pub fn import(&self, bytes: &[u8]) -> Result<(), LoroError> {
        self.import_with(bytes, Default::default())
    }

    pub fn import_with(&self, bytes: &[u8], origin: InternalString) -> Result<(), LoroError> {
        let (magic_bytes, input) = bytes.split_at(4);
        let magic_bytes: [u8; 4] = magic_bytes.try_into().unwrap();
        if magic_bytes != MAGIC_BYTES {
            return Err(LoroError::DecodeError("Invalid header bytes".into()));
        }
        let (version, input) = input.split_at(1);
        if version != [ENCODE_SCHEMA_VERSION] {
            return Err(LoroError::DecodeError("Invalid version".into()));
        }

        let mode: ConcreteEncodeMode = input[0].into();
        match mode {
            ConcreteEncodeMode::Updates | ConcreteEncodeMode::RleUpdates => {
                // TODO: need to throw error if state is in transaction
                debug_log::group!("import");
                let mut oplog = self.oplog.lock().unwrap();
                let old_vv = oplog.vv().clone();
                let old_frontiers = oplog.frontiers().clone();
                oplog.decode(bytes)?;
                let mut diff = DiffCalculator::new();
                let diff = diff.calc_diff_internal(
                    &oplog,
                    &old_vv,
                    Some(&old_frontiers),
                    oplog.vv(),
                    Some(oplog.dag.get_frontiers()),
                );
                if !self.detached {
                    let mut state = self.state.lock().unwrap();
                    state.apply_diff(InternalDocDiff {
                        origin,
                        local: false,
                        diff: (&diff).into(),
                        new_version: Cow::Borrowed(oplog.frontiers()),
                    });
                }

                debug_log::group_end!();
            }
            ConcreteEncodeMode::Snapshot => {
                if self.is_empty() {
                    decode_app_snapshot(self, &input[1..])?;
                } else {
                    let app = LoroDoc::new();
                    decode_app_snapshot(&app, &input[1..])?;
                    let oplog = self.oplog.lock().unwrap();
                    let updates = app.export_from(oplog.vv());
                    drop(oplog);
                    return self.import_with(&updates, origin);
                }
            }
        };

        debug_dbg!(&self.oplog.lock().unwrap().changes);
        self.emit_events();

        Ok(())
    }

    fn emit_events(&self) {
        let mut state = self.state.lock().unwrap();
        for event in state.take_events() {
            self.observer.emit(event);
        }
    }

    pub fn export_snapshot(&self) -> Vec<u8> {
        debug_log::group!("export snapshot");
        debug_dbg!(&self.oplog.lock().unwrap().changes);
        debug_dbg!(&self.state.lock().unwrap().get_deep_value());
        let version = ENCODE_SCHEMA_VERSION;
        let mut ans = Vec::from(MAGIC_BYTES);
        // maybe u8 is enough
        ans.push(version);
        ans.push((EncodeMode::Snapshot).to_byte());
        ans.extend(encode_app_snapshot(self));
        debug_log::group_end!();
        ans
    }

    pub fn vv_cloned(&self) -> VersionVector {
        self.oplog.lock().unwrap().vv().clone()
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_text<I: Into<ContainerIdRaw>>(&self, id: I) -> TextHandler {
        let idx = self.get_container_idx(id, ContainerType::Text);
        TextHandler::new(idx, Arc::downgrade(&self.state))
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_list<I: Into<ContainerIdRaw>>(&self, id: I) -> ListHandler {
        let idx = self.get_container_idx(id, ContainerType::List);
        ListHandler::new(idx, Arc::downgrade(&self.state))
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_map<I: Into<ContainerIdRaw>>(&self, id: I) -> MapHandler {
        let idx = self.get_container_idx(id, ContainerType::Map);
        MapHandler::new(idx, Arc::downgrade(&self.state))
    }

    pub fn diagnose_size(&self) {
        self.oplog().lock().unwrap().diagnose_size();
    }

    fn get_container_idx<I: Into<ContainerIdRaw>>(
        &self,
        id: I,
        c_type: ContainerType,
    ) -> ContainerIdx {
        let id: ContainerIdRaw = id.into();
        match id {
            ContainerIdRaw::Root { name } => self.oplog().lock().unwrap().arena.register_container(
                &crate::container::ContainerID::Root {
                    name,
                    container_type: c_type,
                },
            ),
            ContainerIdRaw::Normal { id: _ } => self
                .oplog()
                .lock()
                .unwrap()
                .arena
                .register_container(&id.with_type(c_type)),
        }
    }

    pub fn subscribe_deep(&self, callback: Subscriber) -> SubID {
        let mut state = self.state.lock().unwrap();
        if !state.is_recording() {
            state.start_recording();
        }

        self.observer.subscribe_deep(callback)
    }

    pub fn subscribe(&self, container_id: &ContainerID, callback: Subscriber) -> SubID {
        let mut state = self.state.lock().unwrap();
        if !state.is_recording() {
            state.start_recording();
        }

        self.observer.subscribe(container_id, callback)
    }

    pub fn unsubscribe(&self, id: SubID) {
        self.observer.unsubscribe(id);
    }
}

impl Default for LoroDoc {
    fn default() -> Self {
        Self::new()
    }
}
