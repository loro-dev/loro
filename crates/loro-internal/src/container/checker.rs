use fxhash::FxHashSet;

use crate::delta::DeltaItem;
use crate::{container::registry::ContainerIdx, InternalString, LoroError};

use crate::transaction::op::{ListTxnOps, MapTxnOps, TextTxnOps};

/// [ListChecker] maintains the length of all list container during one transaction,
/// when a op is be inserted, it will check whether the position or the length of deletion is valid.
#[derive(Debug, Clone)]
pub(crate) struct ListChecker {
    pub(crate) idx: ContainerIdx,
    pub(crate) current_length: usize,
}

#[derive(Debug, Clone)]
pub(super) struct MapChecker {
    pub(crate) idx: ContainerIdx,
    pub(crate) keys: FxHashSet<InternalString>,
}

#[derive(Debug, Clone)]
pub(super) struct TextChecker {
    pub(crate) idx: ContainerIdx,
    // TODO rope?
    pub(crate) current_length: usize,
}

impl ListChecker {
    pub(crate) fn from_idx(idx: ContainerIdx) -> Self {
        Self {
            idx,
            current_length: 0,
        }
    }

    pub(crate) fn new(idx: ContainerIdx, current_length: usize) -> Self {
        Self {
            idx,
            current_length,
        }
    }

    pub(crate) fn check(&mut self, ops: &ListTxnOps) -> Result<(), LoroError> {
        let mut index = 0;
        for op in ops.items() {
            match op {
                DeltaItem::Insert { value, .. } => {
                    index += value.len();
                    self.current_length += value.len()
                }
                DeltaItem::Retain { len, .. } => {
                    index += len;
                    if *len > self.current_length {
                        return Err(LoroError::TransactionError(
                            format!("`List-{:?}` index out of bounds: the len is {} but the index is {}", self.idx, self.current_length, len).into(),
                        ));
                    }
                }
                DeltaItem::Delete(l) => {
                    if index + *l > self.current_length {
                        return Err(LoroError::TransactionError(
                            format!("`List-{:?}` can not apply delete op: the current len is {} but the delete range is {:?}", self.idx, self.current_length, index..index+*l).into(),
                        ));
                    }
                    self.current_length -= *l;
                }
            }
        }
        Ok(())
    }
}

impl TextChecker {
    pub(crate) fn new(idx: ContainerIdx, current_length: usize) -> Self {
        Self {
            idx,
            current_length,
        }
    }

    pub(crate) fn from_idx(idx: ContainerIdx) -> Self {
        Self {
            idx,
            current_length: 0,
        }
    }
    pub(crate) fn check(&mut self, ops: &TextTxnOps) -> Result<(), LoroError> {
        // TODO utf-16
        let mut index = 0;
        for op in ops.items() {
            match op {
                DeltaItem::Insert { value, .. } => {
                    index += value.len();
                    self.current_length += value.len()
                }
                DeltaItem::Retain { len, .. } => {
                    index += len;
                    if *len > self.current_length {
                        return Err(LoroError::TransactionError(
                            format!("`Text-{:?}` index out of bounds: the len is {} but the index is {}", self.idx, self.current_length, len).into(),
                        ));
                    }
                }
                DeltaItem::Delete(l) => {
                    if index + *l > self.current_length {
                        return Err(LoroError::TransactionError(
                            format!("`Text-{:?}` can not apply delete op: the current len is {} but the delete range is {:?}", self.idx, self.current_length, index..index+*l).into(),
                        ));
                    }
                    self.current_length -= *l;
                }
            }
        }
        Ok(())
    }
}

impl MapChecker {
    pub(crate) fn new(idx: ContainerIdx, keys: FxHashSet<InternalString>) -> Self {
        Self { idx, keys }
    }

    pub(crate) fn from_idx(idx: ContainerIdx) -> Self {
        Self {
            idx,
            keys: Default::default(),
        }
    }
    pub(crate) fn check(&mut self, ops: &MapTxnOps) -> Result<(), LoroError> {
        self.keys.extend(ops.added.keys().cloned());
        self.keys.retain(|k| !ops.deleted.contains(k));
        Ok(())
    }
}
