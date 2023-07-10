use std::{
    mem::take,
    sync::{Arc, Mutex},
};

use rle::RleVec;

use crate::{
    change::Change,
    container::ContainerID,
    op::{Op, RemoteContent, RemoteOp},
    version::Frontiers,
    LoroError,
};

use super::{arena::SharedArena, oplog::OpLog, state::AppState};

pub struct Transaction {
    finished: bool,
    state: Arc<Mutex<AppState>>,
    oplog: Arc<Mutex<OpLog>>,
    next_frontiers: Frontiers,
    local_ops: RleVec<[Op; 1]>,
    arena: SharedArena,
}

impl Transaction {
    pub fn new(state: Arc<Mutex<AppState>>, oplog: Arc<Mutex<OpLog>>) -> Self {
        let mut state_lock = state.lock().unwrap();
        state_lock.start_txn();
        let arena = state_lock.arena.clone();
        let frontiers = state_lock.frontiers.clone();
        drop(state_lock);
        Self {
            state,
            arena,
            oplog,
            next_frontiers: frontiers,
            finished: false,
            local_ops: RleVec::new(),
        }
    }

    pub fn abort(&mut self) {
        self.state.lock().unwrap().abort_txn();
        self.finished = true;
    }

    pub fn commit(&mut self, oplog: &mut OpLog) -> Result<(), LoroError> {
        let mut state = self.state.lock().unwrap();
        let ops = std::mem::take(&mut self.local_ops);
        let change = Change {
            ops,
            deps: state.frontiers.clone(),
            id: oplog.next_id(state.peer),
            lamport: oplog.next_lamport(),
            timestamp: oplog.get_timestamp(),
        };

        if let Err(err) = oplog.import_change(change) {
            drop(state);
            self.abort();
            return Err(err);
        }

        state.commit_txn(take(&mut self.next_frontiers));
        self.finished = true;
        Ok(())
    }

    pub fn import_local_op(&mut self, container: ContainerID, op: RemoteContent) {}
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if !self.finished {
            self.abort();
        }
    }
}
