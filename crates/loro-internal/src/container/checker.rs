use fxhash::FxHashSet;

use crate::{container::registry::ContainerIdx, InternalString, LoroError};

use crate::transaction::op::MapTxnOps;

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

    pub(crate) fn check_insert(&mut self, pos: usize, len: usize) -> Result<(), LoroError> {
        if pos > self.current_length {
            return Err(LoroError::TransactionError(
                format!(
                    "`ContainerIdx-{:?}` index out of bounds: the len is {} but the index is {}",
                    self.idx, self.current_length, pos
                )
                .into(),
            ));
        }
        self.current_length += len;
        Ok(())
    }

    pub(crate) fn check_delete(&mut self, pos: usize, len: usize) -> Result<(), LoroError> {
        if pos > self.current_length {
            return Err(LoroError::TransactionError(
                format!(
                    "`ContainerIdx-{:?}` index out of bounds: the len is {} but the index is {}",
                    self.idx, self.current_length, pos
                )
                .into(),
            ));
        }
        if pos + len > self.current_length {
            return Err(LoroError::TransactionError(
                format!("`ContainerIdx-{:?}` can not apply delete op: the current len is {} but the delete range is {:?}", self.idx, self.current_length, pos..pos+len).into(),
            ));
        }
        self.current_length -= len;
        Ok(())
    }
}

// TODO
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
    pub(crate) fn check_insert(&mut self, ops: &MapTxnOps) -> Result<(), LoroError> {
        self.keys.extend(ops.added.keys().cloned());
        self.keys.retain(|k| !ops.deleted.contains(k));
        Ok(())
    }
}
