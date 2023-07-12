use std::{
    mem::take,
    sync::{Arc, Mutex},
};

use rle::{HasLength, RleVec};

use crate::{
    change::{Change, Lamport},
    container::{registry::ContainerIdx, ContainerIdRaw},
    id::{Counter, PeerID, ID},
    op::{Op, RawOp, RawOpContent},
    span::HasIdSpan,
    version::Frontiers,
    LoroError, LoroValue,
};

use super::{arena::SharedArena, handler::Text, oplog::OpLog, state::AppState};

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
        self.local_ops.clear();
        self.finished = true;
    }

    pub fn commit(&mut self) -> Result<(), LoroError> {
        let mut state = self.state.lock().unwrap();
        if self.local_ops.is_empty() {
            state.abort_txn();
            self.finished = true;
            return Ok(());
        }

        let ops = std::mem::take(&mut self.local_ops);
        let mut oplog = self.oplog.lock().unwrap();
        let deps = take(&mut self.frontiers);
        let change = Change {
            ops,
            deps,
            id: oplog.next_id(state.peer),
            lamport: oplog.next_lamport(),
            timestamp: oplog.get_timestamp(),
        };

        let last_id = change.id_last();
        if let Err(err) = oplog.import_local_change(change) {
            drop(state);
            drop(oplog);
            self.abort();
            return Err(err);
        }
        state.commit_txn(
            Frontiers::from_id(last_id),
            self.next_lamport,
            self.next_counter,
        );
        self.finished = true;
        Ok(())
    }

    pub fn apply_local_op(&mut self, container: ContainerIdx, content: RawOpContent) {
        let len = content.content_len();
        let op = RawOp {
            id: ID {
                peer: self.peer,
                counter: self.next_counter,
            },
            lamport: self.next_lamport,
            container,
            content,
        };
        self.push_local_op_to_log(&op);
        let mut state = self.state.lock().unwrap();
        state.apply_local_op(op);
        self.next_counter += len as Counter;
        self.next_lamport += len as Lamport;
    }

    fn push_local_op_to_log(&mut self, op: &RawOp) {
        let op = self.arena.convert_raw_op(op);
        self.local_ops.push(op);
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_text<I: Into<ContainerIdRaw>>(&self, id: I) -> Option<Text> {
        let id: ContainerIdRaw = id.into();
        let idx = match id {
            ContainerIdRaw::Root { name } => Some(self.arena.register_container(
                &crate::container::ContainerID::Root {
                    name,
                    container_type: crate::ContainerType::Text,
                },
            )),
            ContainerIdRaw::Normal { id: _ } => self
                .arena
                .id_to_idx(&id.with_type(crate::ContainerType::Text)),
        };

        idx.map(|x| x.into())
    }

    pub fn get_value_by_idx(&self, idx: ContainerIdx) -> LoroValue {
        self.state.lock().unwrap().get_value_by_idx(idx)
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if !self.finished {
            // TODO: should we abort here or commit here?
            // what if commit fails?
            self.commit().unwrap_or_default();
        }
    }
}
