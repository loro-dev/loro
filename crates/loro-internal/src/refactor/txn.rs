use rle::RleVec;

use crate::{change::Change, op::RemoteOp, LoroError};

use super::{oplog::OpLog, state::AppState};

pub struct Transaction<'a> {
    finished: bool,
    state: &'a mut AppState,
    ops: RleVec<[RemoteOp<'a>; 1]>,
}

impl<'a> Transaction<'a> {
    pub fn new(state: &'a mut AppState) -> Self {
        state.start_txn();
        Self {
            state,
            finished: false,
            ops: RleVec::new(),
        }
    }

    pub fn abort(&mut self) {
        self.state.abort_txn();
        self.finished = true;
    }

    pub fn commit(&mut self, oplog: &mut OpLog) -> Result<(), LoroError> {
        let ops = std::mem::take(&mut self.ops);
        let change = Change {
            ops,
            deps: self.state.frontiers.clone(),
            id: oplog.next_id(self.state.peer),
            lamport: oplog.next_lamport(),
            timestamp: oplog.get_timestamp(),
        };

        if let Err(err) = oplog.import_change(change) {
            self.abort();
            return Err(err);
        }

        self.state.commit_txn();
        self.finished = true;
        Ok(())
    }
}

impl<'a> Drop for Transaction<'a> {
    fn drop(&mut self) {
        if !self.finished {
            self.abort();
        }
    }
}
