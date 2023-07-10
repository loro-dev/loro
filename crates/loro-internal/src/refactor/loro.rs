use std::sync::{Arc, Mutex};

use crate::LoroError;

use super::{oplog::OpLog, state::AppState, txn::Transaction};

pub struct LoroApp {
    oplog: Arc<Mutex<OpLog>>,
    state: Arc<Mutex<AppState>>,
}

impl LoroApp {
    pub fn new() -> Self {
        let oplog = OpLog::new();
        let state = Arc::new(Mutex::new(AppState::new(&oplog)));
        Self {
            oplog: Arc::new(Mutex::new(oplog)),
            state,
        }
    }

    pub fn txn(&self) -> Result<Transaction, LoroError> {
        if self.state.lock().unwrap().is_in_txn() {
            return Err(LoroError::DuplicatedTransactionError);
        }

        Ok(Transaction::new(self.state.clone(), self.oplog.clone()))
    }
}
