use std::sync::{Arc, Mutex};

use crate::{id::PeerID, LoroError, VersionVector};

use super::{
    diff_calc::DiffCalculator,
    oplog::OpLog,
    state::{AppState, AppStateDiff, ContainerStateDiff},
    txn::Transaction,
};

/// `LoroApp` serves as the library's primary entry point.
/// It's constituted by an [OpLog] and an [AppState].
///
/// - [OpLog] encompasses all operations, signifying the document history.
/// - [AppState] signifies the current document state.
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
        let state = Arc::new(Mutex::new(AppState::new(&oplog)));
        Self {
            oplog: Arc::new(Mutex::new(oplog)),
            state,
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
            diff: &diff,
            frontiers: oplog.frontiers(),
            next_lamport: oplog.latest_lamport + 1,
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
                diff: &diff,
                frontiers: oplog.frontiers(),
                next_lamport: oplog.latest_lamport + 1,
            });
        }

        debug_log::group_end!();
        Ok(diff)
    }

    pub fn encode_snapshot(&self) -> Vec<u8> {
        unimplemented!();
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
