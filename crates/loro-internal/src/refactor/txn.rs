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

use super::{
    arena::SharedArena,
    handler::{ListHandler, MapHandler, TextHandler},
    oplog::OpLog,
    state::{AppState, State},
};

pub struct Transaction {
    peer: PeerID,
    next_counter: Counter,
    next_lamport: Lamport,
    state: Arc<Mutex<AppState>>,
    oplog: Arc<Mutex<OpLog>>,
    frontiers: Frontiers,
    local_ops: RleVec<[Op; 1]>,
    arena: SharedArena,
    finished: bool,
}

impl Transaction {
    pub fn new(state: Arc<Mutex<AppState>>, oplog: Arc<Mutex<OpLog>>) -> Self {
        let mut state_lock = state.lock().unwrap();
        state_lock.start_txn();
        let arena = state_lock.arena.clone();
        let frontiers = state_lock.frontiers.clone();
        let peer = state_lock.peer;
        let next_counter = state_lock.next_counter;
        let next_lamport = oplog
            .lock()
            .unwrap()
            .dag
            .frontiers_to_next_lamport(&frontiers);
        drop(state_lock);
        Self {
            peer,
            next_counter,
            state,
            arena,
            oplog,
            next_lamport,
            frontiers,
            local_ops: RleVec::new(),
            finished: false,
        }
    }

    pub fn abort(mut self) {
        self._abort();
    }

    fn _abort(&mut self) {
        if self.finished {
            return;
        }

        self.finished = true;
        self.state.lock().unwrap().abort_txn();
        self.local_ops.clear();
    }

    pub fn commit(mut self) -> Result<(), LoroError> {
        self._commit()
    }

    fn _commit(&mut self) -> Result<(), LoroError> {
        if self.finished {
            return Ok(());
        }

        self.finished = true;
        let mut state = self.state.lock().unwrap();
        if self.local_ops.is_empty() {
            state.abort_txn();
            return Ok(());
        }

        let ops = std::mem::take(&mut self.local_ops);
        let mut oplog = self.oplog.lock().unwrap();
        let deps = take(&mut self.frontiers);
        let change = Change {
            lamport: self.next_lamport - ops.atom_len() as Lamport,
            ops,
            deps,
            id: oplog.next_id(state.peer),
            timestamp: oplog.get_timestamp(),
        };

        let last_id = change.id_last();
        if let Err(err) = oplog.import_local_change(change) {
            drop(state);
            drop(oplog);
            self._abort();
            return Err(err);
        }
        state.commit_txn(Frontiers::from_id(last_id), self.next_counter);
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
    pub fn get_text<I: Into<ContainerIdRaw>>(&self, id: I) -> Option<TextHandler> {
        let idx = self.get_container_idx(id);
        idx.map(|x| TextHandler::new(x, Arc::downgrade(&self.state)))
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_list<I: Into<ContainerIdRaw>>(&self, id: I) -> Option<ListHandler> {
        let idx = self.get_container_idx(id);
        idx.map(|x| ListHandler::new(x, Arc::downgrade(&self.state)))
    }

    /// id can be a str, ContainerID, or ContainerIdRaw.
    /// if it's str it will use Root container, which will not be None
    pub fn get_map<I: Into<ContainerIdRaw>>(&self, id: I) -> Option<MapHandler> {
        let idx = self.get_container_idx(id);
        idx.map(|x| MapHandler::new(x, Arc::downgrade(&self.state)))
    }

    fn get_container_idx<I: Into<ContainerIdRaw>>(&self, id: I) -> Option<ContainerIdx> {
        let id: ContainerIdRaw = id.into();
        match id {
            ContainerIdRaw::Root { name } => Some(self.arena.register_container(
                &crate::container::ContainerID::Root {
                    name,
                    container_type: crate::ContainerType::Text,
                },
            )),
            ContainerIdRaw::Normal { id: _ } => self
                .arena
                .id_to_idx(&id.with_type(crate::ContainerType::Text)),
        }
    }

    pub fn get_value_by_idx(&self, idx: ContainerIdx) -> LoroValue {
        self.state.lock().unwrap().get_value_by_idx(idx)
    }

    pub(crate) fn with_state<F, R>(&self, idx: ContainerIdx, f: F) -> R
    where
        F: FnOnce(&State) -> R,
    {
        let state = self.state.lock().unwrap();
        f(state.get_state(idx).unwrap())
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if !self.finished {
            // TODO: should we abort here or commit here?
            // what if commit fails?
            self._commit().unwrap_or_default();
        }
    }
}
