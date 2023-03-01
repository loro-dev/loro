use enum_as_inner::EnumAsInner;
use smallvec::SmallVec;

use crate::{
    container::{registry::ContainerIdx, ContainerID},
    ContainerType, InternalString, LoroError, LoroValue,
};

use super::Transaction;

#[derive(Debug, EnumAsInner)]
pub enum TransactionOp {
    List {
        container: ContainerIdx,
        op: ListTxnOp,
    },
    Map {
        container: ContainerIdx,
        op: MapTxnOp,
    },
    Text {
        container: ContainerIdx,
        op: TextTxnOp,
    },
}

#[derive(Debug)]
pub enum TextTxnOp {
    Insert { pos: usize, text: Box<str> },
    Delete { pos: usize, len: usize },
}

#[derive(Debug)]
pub enum MapTxnOp {
    Insert {
        key: InternalString,
        value: LoroValue,
    },
    InsertContainer {
        key: InternalString,
        type_: ContainerType,
        container: Option<ContainerIdx>,
    },
    Delete {
        key: InternalString,
        deleted_container: Option<ContainerIdx>,
    },
}

#[derive(Debug)]
pub enum ListTxnOp {
    InsertValue {
        pos: usize,
        value: LoroValue,
    },
    InsertBatchValue {
        pos: usize,
        values: Vec<LoroValue>,
    },
    InsertContainer {
        pos: usize,
        type_: ContainerType,
        // The ContainerIdx will be create by Transaction
        // And when the transaction applies the op, it will be converted to real ContainerID and the op will be merged into [Self::InsertBatchValue]
        container: Option<ContainerIdx>,
    },
    Delete {
        pos: usize,
        len: usize,
        
        deleted_container: Option<SmallVec<[ContainerIdx; 1]>>,
    },
}

// TODO: builder?
impl TransactionOp {
    pub(crate) fn container_idx(&self) -> ContainerIdx {
        match self {
            Self::List { container, .. } => *container,
            Self::Map { container, .. } => *container,
            Self::Text { container, .. } => *container,
        }
    }

    pub(crate) fn insert_text(container: ContainerIdx, pos: usize, text: &str) -> Self {
        Self::Text {
            container,
            op: TextTxnOp::Insert {
                pos,
                text: text.into(),
            },
        }
    }

    pub(crate) fn delete_text(container: ContainerIdx, pos: usize, len: usize) -> Self {
        Self::Text {
            container,
            op: TextTxnOp::Delete { pos, len },
        }
    }

    pub(crate) fn insert_map_value(
        container: ContainerIdx,
        key: InternalString,
        value: LoroValue,
    ) -> Self {
        Self::Map {
            container,
            op: MapTxnOp::Insert { key, value },
        }
    }

    pub(crate) fn insert_map_container(
        container: ContainerIdx,
        key: InternalString,
        type_: ContainerType,
    ) -> Self {
        TransactionOp::Map {
            container,
            op: MapTxnOp::InsertContainer {
                key,
                type_,
                container: None,
            },
        }
    }

    pub(crate) fn delete_map(
        container: ContainerIdx,
        key: InternalString,
        deleted_container: Option<ContainerIdx>,
    ) -> Self {
        TransactionOp::Map {
            container,
            op: MapTxnOp::Delete {
                key,
                deleted_container,
            },
        }
    }

    pub(crate) fn insert_list_value(container: ContainerIdx, pos: usize, value: LoroValue) -> Self {
        Self::List {
            container,
            op: ListTxnOp::InsertValue { pos, value },
        }
    }

    pub(crate) fn insert_list_batch_value(
        container: ContainerIdx,
        pos: usize,
        values: Vec<LoroValue>,
    ) -> Self {
        Self::List {
            container,
            op: ListTxnOp::InsertBatchValue { pos, values },
        }
    }

    pub(crate) fn insert_list_container(
        container: ContainerIdx,
        pos: usize,
        type_: ContainerType,
    ) -> Self {
        Self::List {
            container,
            op: ListTxnOp::InsertContainer {
                pos,
                type_,
                container: None,
            },
        }
    }

    pub(crate) fn delete_list(
        container: ContainerIdx,
        pos: usize,
        len: usize,
        deleted_container: Option<SmallVec<[ContainerIdx; 1]>>,
    ) -> Self {
        Self::List {
            container,
            op: ListTxnOp::Delete {
                pos,
                len,
                deleted_container,
            },
        }
    }

    pub fn is_insert_container(&self) -> bool {
        match self {
            TransactionOp::List { op, .. } => op.is_insert_container(),
            TransactionOp::Map { op, .. } => op.is_insert_container(),
            _ => false,
        }
    }

    pub fn register_container_and_convert(
        &mut self,
        txn: &mut Transaction,
    ) -> Result<(), LoroError> {
        match self {
            TransactionOp::List { container, op } => {
                let id = op.register_container(txn, *container)?;
                *op = ListTxnOp::InsertValue {
                    pos: op.pos(),
                    value: LoroValue::Unresolved(id.into()),
                };
                Ok(())
            }
            TransactionOp::Map { container, op } => {
                let id = op.register_container(txn, *container)?;
                *op = MapTxnOp::Insert {
                    key: op.key().clone(),
                    value: LoroValue::Unresolved(id.into()),
                };
                Ok(())
            }
            _ => unreachable!("Text cannot insert container"),
        }
    }
}

impl ListTxnOp {
    pub fn is_insert_container(&self) -> bool {
        matches!(self, ListTxnOp::InsertContainer { .. })
    }

    pub fn register_container(
        &self,
        txn: &mut Transaction,
        parent_idx: ContainerIdx,
    ) -> Result<ContainerID, LoroError> {
        match self {
            ListTxnOp::InsertContainer {
                type_, container, ..
            } => Ok(txn.register_container(container.unwrap(), *type_, parent_idx)),
            _ => Err(LoroError::TransactionError(
                "not insert container op".into(),
            )),
        }
    }

    pub fn pos(&self) -> usize {
        match self {
            ListTxnOp::InsertValue { pos, .. } => *pos,
            ListTxnOp::Delete { pos, .. } => *pos,
            ListTxnOp::InsertBatchValue { pos, .. } => *pos,
            ListTxnOp::InsertContainer { pos, .. } => *pos,
        }
    }
}

impl MapTxnOp {
    pub fn key(&self) -> &InternalString {
        match self {
            MapTxnOp::Insert { key, .. } => key,
            MapTxnOp::InsertContainer { key, .. } => key,
            MapTxnOp::Delete { key, .. } => key,
        }
    }

    pub fn is_insert_container(&self) -> bool {
        matches!(self, Self::InsertContainer { .. })
    }

    pub fn register_container(
        &self,
        txn: &mut Transaction,
        parent_idx: ContainerIdx,
    ) -> Result<ContainerID, LoroError> {
        match self {
            MapTxnOp::InsertContainer {
                type_, container, ..
            } => Ok(txn.register_container(container.unwrap(), *type_, parent_idx)),
            _ => Err(LoroError::TransactionError(
                "not insert container op".into(),
            )),
        }
    }
}
