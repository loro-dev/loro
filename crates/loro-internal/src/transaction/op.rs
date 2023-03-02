use enum_as_inner::EnumAsInner;

use crate::{
    container::registry::ContainerIdx,
    delta::{DeltaItem, MapDiff, MapDiffRaw, Meta, SeqDelta},
    ContainerType, InternalString, LoroValue, Map,
};

pub(crate) type ListTxnOps = SeqDelta<Vec<Value>>;
pub(crate) type TextTxnOps = SeqDelta<String>;
pub(crate) type MapTxnOps = MapDiffRaw<Value>;

impl MapTxnOps {
    pub(super) fn into_event_format(self, map_container: &Map) -> MapDiff {
        let mut ans = MapDiff::default();
        for (k, v) in self.added.into_iter() {
            let v = v.into_value().unwrap();
            if let Some(old) = map_container.get(&k) {
                ans.updated.insert(k, (old, v).into());
            } else {
                ans.added.insert(k, v);
            }
        }
        for k in self.deleted {
            ans.deleted.insert(k);
        }
        ans
    }
}

impl ListTxnOps {
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
        ops: ListTxnOps,
    },
    Map {
        container: ContainerIdx,
        ops: MapTxnOps,
    },
    Text {
        container: ContainerIdx,
        ops: TextTxnOps,
    },
}

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

    pub(crate) fn list_inner(self) -> ListTxnOps {
        if let TransactionOp::List { ops, .. } = self {
            ops
        } else {
            unreachable!()
        }
    }

    pub(crate) fn text_inner(self) -> TextTxnOps {
        if let TransactionOp::Text { ops, .. } = self {
            ops
        } else {
            unreachable!()
        }
    }

    pub(crate) fn map_inner(self) -> MapTxnOps {
        if let TransactionOp::Map { ops, .. } = self {
            ops
        } else {
            unreachable!()
        }
    }

    pub(crate) fn insert_text(container: ContainerIdx, pos: usize, text: String) -> Self {
        Self::Text {
            container,
            ops: TextTxnOps::new().retain(pos).insert(text),
        }
    }

    pub(crate) fn delete_text(container: ContainerIdx, pos: usize, len: usize) -> Self {
        Self::Text {
            container,
            ops: TextTxnOps::new().retain(pos).delete(len),
        }
    }

    pub(crate) fn insert_map_value(
        container: ContainerIdx,
        key: InternalString,
        value: LoroValue,
    ) -> Self {
        Self::Map {
            container,
            ops: MapTxnOps::new().insert(key, value.into()),
        }
    }

    pub(crate) fn insert_map_container(
        container: ContainerIdx,
        key: InternalString,
        type_: ContainerType,
        idx: ContainerIdx,
    ) -> Self {
        TransactionOp::Map {
            container,
            ops: MapTxnOps::new().insert(key, Value::Container((type_, idx))),
        }
    }

    pub(crate) fn delete_map(container: ContainerIdx, key: &InternalString) -> Self {
        TransactionOp::Map {
            container,
            ops: MapTxnOps::new().delete(key),
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
                .insert(vec![Value::Container((type_, idx))]),
        }
    }

    pub(crate) fn delete_list(container: ContainerIdx, pos: usize, len: usize) -> Self {
        Self::List {
            container,
            ops: SeqDelta::new().retain(pos).delete(len),
        }
    }
}
