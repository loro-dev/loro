use fxhash::{FxHashMap, FxHashSet};

use crate::{container::registry::ContainerIdx, delta::SeqDelta, InternalString, LoroError};

use super::op::{MapTxnOp, TextTxnOp, TransactionOp, Value};

/// [ListChecker] maintains the length of all list container during one transaction,
/// when a op is be inserted, it will check whether the position or the length of deletion is valid.
#[derive(Debug, Default)]
pub(super) struct ListChecker {
    current_length: FxHashMap<ContainerIdx, Option<usize>>,
}

impl ListChecker {
    pub(super) fn check(&mut self, op: &SeqDelta<Vec<Value>>) -> Result<(), LoroError> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub(super) struct TextChecker {
    current_length: FxHashMap<ContainerIdx, Option<usize>>,
}

impl TextChecker {
    pub(super) fn check(&mut self, op: &TextTxnOp) -> Result<(), LoroError> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub(super) struct MapChecker {
    keys: FxHashMap<ContainerIdx, FxHashSet<InternalString>>,
}

impl MapChecker {
    pub(super) fn check(&mut self, op: &MapTxnOp) -> Result<(), LoroError> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub(super) struct Checker {
    list: ListChecker,
    map: MapChecker,
    // TODO: utf-16?
    text: TextChecker,
}

impl Checker {
    pub(super) fn check(&mut self, op: &TransactionOp) -> Result<(), LoroError> {
        match op {
            TransactionOp::List { container, ops: op } => self.list.check(op),
            TransactionOp::Map { container, op } => self.map.check(op),
            TransactionOp::Text { container, op } => self.text.check(op),
        }
    }
}
