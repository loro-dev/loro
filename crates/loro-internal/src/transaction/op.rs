use enum_as_inner::EnumAsInner;

use crate::{
    container::{registry::ContainerIdx, ContainerID},
    delta::{DeltaItem, Meta, SeqDelta},
    ContainerType, InternalString, LoroError, LoroValue,
};

use super::Transaction;

pub(crate) type ListTxnOp = SeqDelta<Vec<Value>>;

impl ListTxnOp {
    pub(super) fn into_event_format(self) -> SeqDelta<Vec<LoroValue>> {
        let items = self
            .inner()
            .into_iter()
            .map(|item| item.into_event_format())
            .collect();
        SeqDelta { vec: items }
    }
}

impl<M: Meta> DeltaItem<Vec<Value>, M> {
    pub(crate) fn into_event_format(self) -> DeltaItem<Vec<LoroValue>, M> {
        match self {
            DeltaItem::Delete(l) => DeltaItem::Delete(l),
            DeltaItem::Retain { len, meta } => DeltaItem::Retain { len, meta },
            DeltaItem::Insert { value, meta } => DeltaItem::Insert {
                value: value.into_iter().map(|v| v.into_value().unwrap()).collect(),
                meta,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, EnumAsInner)]
pub enum Value {
    Value(LoroValue),
    Container((ContainerType, ContainerIdx)),
}

impl From<LoroValue> for Value {
    fn from(value: LoroValue) -> Self {
        Self::Value(value)
    }
}

impl From<(ContainerType, ContainerIdx)> for Value {
    fn from(value: (ContainerType, ContainerIdx)) -> Self {
        Self::Container(value)
    }
}

#[derive(Debug, EnumAsInner)]
pub enum TransactionOp {
    List {
        container: ContainerIdx,
        ops: ListTxnOp,
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

// TODO: builder?
impl TransactionOp {
    pub(crate) fn container_idx(&self) -> ContainerIdx {
        match self {
            Self::List { container, .. } => *container,
            Self::Map { container, .. } => *container,
            Self::Text { container, .. } => *container,
        }
    }

    pub(crate) fn container_type(&self) -> ContainerType {
        match self {
            Self::List { .. } => ContainerType::List,
            Self::Map { .. } => ContainerType::Map,
            Self::Text { .. } => ContainerType::Text,
        }
    }

    pub(crate) fn list_inner(self) -> ListTxnOp {
        if let TransactionOp::List { container, ops: op } = self {
            op
        } else {
            unreachable!()
        }
    }

    pub(crate) fn list_op_mut(&mut self) -> &mut ListTxnOp {
        if let TransactionOp::List { container, ops: op } = self {
            op
        } else {
            unreachable!()
        }
    }

    pub(crate) fn has_insert_container(&self) -> bool {
        match self {
            Self::List { ops: op, .. } => op.items().iter().any(|op| {
                op.as_insert()
                    .and_then(|(vs, _)| {
                        vs.iter()
                            .any(|v| matches!(v, Value::Container(_)))
                            .then_some(0)
                    })
                    .is_some()
            }),
            _ => unimplemented!(),
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
            ops: SeqDelta::new().retain(pos).insert(vec![Value::from(value)]),
        }
    }

    pub(crate) fn insert_list_batch_value(
        container: ContainerIdx,
        pos: usize,
        values: Vec<LoroValue>,
    ) -> Self {
        Self::List {
            container,
            ops: SeqDelta::new()
                .retain(pos)
                .insert(values.into_iter().map(|v| v.into()).collect()),
        }
    }

    pub(crate) fn insert_list_container(
        container: ContainerIdx,
        pos: usize,
        type_: ContainerType,
        idx: ContainerIdx,
    ) -> Self {
        Self::List {
            container,
            ops: SeqDelta::new()
                .retain(pos)
                .insert(vec![(type_, idx).into()]),
        }
    }

    pub(crate) fn delete_list(container: ContainerIdx, pos: usize, len: usize) -> Self {
        Self::List {
            container,
            ops: SeqDelta::new().retain(pos).delete(len),
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

    pub(crate) fn register_container(
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
