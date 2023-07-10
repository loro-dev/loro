use std::{
    mem::take,
    sync::{Arc, Mutex},
};

use rle::RleVec;

use crate::{
    change::{Change, Lamport},
    container::registry::ContainerIdx,
    id::{Counter, PeerID, ID},
    op::{Op, RawOp, RawOpContent},
    version::Frontiers,
    LoroError,
};

use super::{arena::SharedArena, oplog::OpLog, state::AppState};

pub struct Transaction {
    peer: PeerID,
    next_counter: Counter,
    next_lamport: Lamport,
    finished: bool,
    state: Arc<Mutex<AppState>>,
    oplog: Arc<Mutex<OpLog>>,
    frontiers: Frontiers,
    local_ops: RleVec<[Op; 1]>,
    arena: SharedArena,
}

impl Transaction {
    pub fn new(state: Arc<Mutex<AppState>>, oplog: Arc<Mutex<OpLog>>) -> Self {
        let mut state_lock = state.lock().unwrap();
        state_lock.start_txn();
        let arena = state_lock.arena.clone();
        let frontiers = state_lock.frontiers.clone();
        let peer = state_lock.peer;
        let next_counter = state_lock.next_counter;
        let next_lamport = state_lock.next_lamport;
        drop(state_lock);
        Self {
            peer,
            next_lamport,
            next_counter,
            state,
            arena,
            oplog,
            frontiers,
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

        state.commit_txn(take(&mut self.frontiers));
        self.finished = true;
        Ok(())
    }

    pub fn apply_local_op(&mut self, container: ContainerIdx, content: RawOpContent) {
        let mut state = self.state.lock().unwrap();
        state.apply_local_op(RawOp {
            id: ID {
                peer: self.peer,
                counter: self.next_counter,
            },
            lamport: self.next_lamport,
            container,
            content,
        });
        self.next_counter += 1;
        self.next_lamport += 1;
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if !self.finished {
            self.abort();
        }
    }
}
