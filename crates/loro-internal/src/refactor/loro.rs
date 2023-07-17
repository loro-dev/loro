use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

use crate::{
    id::PeerID,
    log_store::encoding::{ConcreteEncodeMode, ENCODE_SCHEMA_VERSION, MAGIC_BYTES},
    EncodeMode, LoroError, VersionVector,
};

use super::{
    diff_calc::DiffCalculator,
    oplog::OpLog,
    snapshot_encode::{decode_app_snapshot, encode_app_snapshot},
    state::{AppState, AppStateDiff, ContainerStateDiff},
    txn::Transaction,
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
pub struct LoroApp {
    oplog: Arc<Mutex<OpLog>>,
    state: Arc<Mutex<AppState>>,
    detached: bool,
}

impl LoroApp {
    pub fn new() -> Self {
        let oplog = OpLog::new();
        // share arena
        let state = Arc::new(Mutex::new(AppState::new(&oplog)));
        Self {
            oplog: Arc::new(Mutex::new(oplog)),
            state,
            detached: false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.oplog.lock().unwrap().is_empty() && self.state.lock().unwrap().is_empty()
    }

    pub(super) fn from_existing(oplog: OpLog, state: AppState) -> Self {
        Self {
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
        state.apply_diff(AppStateDiff {
            diff: (&diff).into(),
            frontiers: Cow::Borrowed(oplog.frontiers()),
        });
    }

    pub fn txn(&self) -> Result<Transaction, LoroError> {
        if self.state.lock().unwrap().is_in_txn() {
            return Err(LoroError::DuplicatedTransactionError);
        }

        Ok(Transaction::new(self.state.clone(), self.oplog.clone()))
    }

    pub fn app_state(&self) -> &Arc<Mutex<AppState>> {
        &self.state
    }

    pub fn oplog(&self) -> &Arc<Mutex<OpLog>> {
        &self.oplog
    }

    pub fn export_from(&self, vv: &VersionVector) -> Vec<u8> {
        self.oplog.lock().unwrap().export_from(vv)
    }

    pub fn import(&self, bytes: &[u8]) -> Result<Vec<ContainerStateDiff>, LoroError> {
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
                    state.apply_diff(AppStateDiff {
                        diff: (&diff).into(),
                        frontiers: Cow::Borrowed(oplog.frontiers()),
                    });
                }

                debug_log::group_end!();
                Ok(diff)
            }
            ConcreteEncodeMode::Snapshot => {
                if self.is_empty() {
                    decode_app_snapshot(self, &input[1..])?;
                    Ok(vec![]) // TODO: return diff
                } else {
                    let app = LoroApp::new();
                    decode_app_snapshot(&app, &input[1..])?;
                    let oplog = self.oplog.lock().unwrap();
                    let updates = app.export_from(oplog.vv());
                    drop(oplog);
                    self.import(&updates)
                }
            }
        }
    }

    pub fn export_snapshot(&self) -> Vec<u8> {
        let version = ENCODE_SCHEMA_VERSION;
        let mut ans = Vec::from(MAGIC_BYTES);
        // maybe u8 is enough
        ans.push(version);
        ans.push((EncodeMode::Snapshot).to_byte());
        ans.extend(encode_app_snapshot(self));
        ans
    }

    pub(crate) fn vv_cloned(&self) -> VersionVector {
        self.oplog.lock().unwrap().vv().clone()
    }
}

impl Default for LoroApp {
    fn default() -> Self {
        Self::new()
    }
}
