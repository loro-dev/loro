use std::{
    borrow::Cow,
    mem::take,
    sync::{Arc, Mutex},
};

use fxhash::FxHashMap;
use loro_common::ContainerType;
use rle::{HasLength, RleVec};
use smallvec::smallvec;

use crate::{
    change::{Change, Lamport},
    container::{
        list::list_op::InnerListOp, registry::ContainerIdx, text::text_content::SliceRanges,
        ContainerIdRaw,
    },
    delta::{Delta, MapValue},
    event::Diff,
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
    state::{ContainerStateDiff, DocState, State},
};

pub struct Transaction {
    peer: PeerID,
    start_counter: Counter,
    next_counter: Counter,
    start_lamport: Lamport,
    next_lamport: Lamport,
    state: Arc<Mutex<DocState>>,
    oplog: Arc<Mutex<OpLog>>,
    frontiers: Frontiers,
    local_ops: RleVec<[Op; 1]>,
    pub(super) arena: SharedArena,
    finished: bool,
}

impl Transaction {
    pub fn new(state: Arc<Mutex<DocState>>, oplog: Arc<Mutex<OpLog>>) -> Self {
        let mut state_lock = state.lock().unwrap();
        let oplog_lock = oplog.lock().unwrap();
        state_lock.start_txn();
        let arena = state_lock.arena.clone();
        let frontiers = state_lock.frontiers.clone();
        let peer = state_lock.peer;
        let next_counter = oplog_lock.next_id(peer).counter;
        let next_lamport = oplog_lock.dag.frontiers_to_next_lamport(&frontiers);
        drop(state_lock);
        drop(oplog_lock);
        Self {
            peer,
            start_counter: next_counter,
            start_lamport: next_lamport,
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
            lamport: self.start_lamport,
            ops,
            deps,
            id: ID::new(self.peer, self.start_counter),
            timestamp: oplog.get_timestamp(),
        };

        let diff = if state.is_recording() {
            Some(change_to_diff(&change, &oplog.arena))
        } else {
            None
        };

        let last_id = change.id_last();
        if let Err(err) = oplog.import_local_change(change) {
            drop(state);
            drop(oplog);
            self._abort();
            return Err(err);
        }

        state.commit_txn(
            Frontiers::from_id(last_id),
            diff.map(|x| super::state::AppStateDiff {
                diff: Cow::Owned(x),
                new_version: Cow::Borrowed(oplog.frontiers()),
                old_version: None,
            }),
        );
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

    fn get_container_idx<I: Into<ContainerIdRaw>>(
        &self,
        id: I,
        c_type: ContainerType,
    ) -> ContainerIdx {
        let id: ContainerIdRaw = id.into();
        match id {
            ContainerIdRaw::Root { name } => {
                self.arena
                    .register_container(&crate::container::ContainerID::Root {
                        name,
                        container_type: c_type,
                    })
            }
            ContainerIdRaw::Normal { id: _ } => {
                self.arena.register_container(&id.with_type(c_type))
            }
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

    pub fn next_id(&self) -> ID {
        ID {
            peer: self.peer,
            counter: self.next_counter,
        }
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if !self.finished {
            // TODO: should we abort here or commit here?
            // what if commit fails?
            self._commit().unwrap();
        }
    }
}

// PERF: could be compacter
fn change_to_diff(change: &Change, arena: &SharedArena) -> Vec<ContainerStateDiff> {
    let mut diff = Vec::with_capacity(change.ops.len());
    let peer = change.id.peer;
    let mut lamport = change.lamport;
    for op in change.ops.iter() {
        let counter = op.counter;
        let diff_op = ContainerStateDiff {
            idx: op.container,
            diff: match &op.content {
                crate::op::InnerContent::List(list) => match list {
                    InnerListOp::Insert { slice, pos } => Diff::SeqRaw(
                        Delta::new()
                            .retain(*pos)
                            .insert(SliceRanges(smallvec![slice.clone()])),
                    ),
                    InnerListOp::Delete(del) => Diff::List(
                        Delta::new()
                            .retain(del.pos as usize)
                            .delete(del.len as usize),
                    ),
                },
                crate::op::InnerContent::Map(map) => {
                    let value = arena.get_value(map.value as usize).unwrap();
                    let mut updated: FxHashMap<_, _> = Default::default();
                    updated.insert(
                        map.key.clone(),
                        MapValue {
                            counter,
                            value: Some(value),
                            lamport: (lamport, peer),
                        },
                    );
                    Diff::NewMap(crate::delta::MapDelta { updated })
                }
            },
        };
        lamport += op.content_len() as Lamport;
        diff.push(diff_op);
    }
    diff
}
