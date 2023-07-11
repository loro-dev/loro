use std::sync::{Arc, Mutex};

use crate::{LoroError, VersionVector};

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

    pub fn detach(&mut self) {
        self.detached = true;
    }

    pub fn attach(&mut self) {
        self.detached = false;
        let mut state = self.state.lock().unwrap();
        let oplog = self.oplog.lock().unwrap();
        let state_vv = oplog.dag.frontiers_to_vv(&state.frontiers);
        let mut diff = DiffCalculator::new();
        let diff = diff.calc(&oplog, &state_vv, oplog.vv());
        state.apply_diff(AppStateDiff {
            diff: &diff,
            frontiers: oplog.frontiers(),
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

    pub fn encode_from(&self, vv: &VersionVector) -> Vec<u8> {
        self.oplog.lock().unwrap().encode_from(vv)
    }

    pub fn decode(&self, bytes: &[u8]) -> Result<Vec<ContainerStateDiff>, LoroError> {
        let mut oplog = self.oplog.lock().unwrap();
        let old_vv = oplog.vv().clone();
        oplog.decode(bytes)?;
        let mut diff = DiffCalculator::new();
        let diff = diff.calc(&oplog, &old_vv, oplog.vv());
        if self.detached {
            let mut state = self.state.lock().unwrap();
            state.apply_diff(AppStateDiff {
                diff: &diff,
                frontiers: oplog.frontiers(),
            })
        }

        Ok(diff)
    }

    pub fn encode_snapshot(&self) -> Vec<u8> {
        unimplemented!();
    }
}

impl Default for LoroApp {
    fn default() -> Self {
        Self::new()
    }
}
