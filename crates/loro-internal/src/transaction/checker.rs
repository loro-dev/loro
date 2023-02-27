use fxhash::{FxHashMap, FxHashSet};

use crate::{container::registry::ContainerIdx, InternalString, LoroError};

use super::{
    op::{ListTxnOp, TransactionOp},
    Transaction,
};

/// [ListChecker] maintains the length of all list container during one transaction,
/// when a op is be inserted, it will check whether the position or the length of deletion is valid.
#[derive(Debug, Default)]
pub(super) struct ListChecker {
    current_length: FxHashMap<ContainerIdx, Option<usize>>,
}

impl ListChecker {
    pub(super) fn check(&mut self, op: &ListTxnOp) -> Result<(), LoroError> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub(super) struct Checker {
    list: ListChecker,
}

impl Checker {
    pub(super) fn check(&mut self, op: &TransactionOp) -> Result<(), LoroError> {
        match op {
            TransactionOp::List { container, op } => self.list.check(op),
        }
    }
}
