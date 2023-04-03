use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;

use crate::{
    container::registry::{ContainerIdx, ContainerWrapper},
    transaction::Transaction,
    ContainerType, List, LoroError, LoroValue, Map, Text,
};

/// Prelim is a value that is not yet integrated into the Loro.
pub trait Prelim: Sized {
    /// Convert the value into a [`PrelimValue`].
    /// If the value is preliminary(container-like), return [`PrelimValue::Container`] and `Some(self)`
    /// that means the value needs to be integrated into the Loro by creating another container.
    ///
    /// If the value is not preliminary, return [`PrelimValue::Value`] and `None`. The value will be insert into the container of Loro directly.
    fn convert_value(self) -> Result<(PrelimValue, Option<Self>), LoroError>;

    /// How to integrate the value into the Loro.
    fn integrate(self, txn: &mut Transaction, container_idx: ContainerIdx)
        -> Result<(), LoroError>;
}

#[derive(Debug, EnumAsInner)]
pub enum PrelimValue {
    Value(LoroValue),
    Container(ContainerType),
}

impl<T> Prelim for T
where
    T: Into<LoroValue>,
{
    fn convert_value(self) -> Result<(PrelimValue, Option<Self>), LoroError> {
        let value: LoroValue = self.into();
        if let LoroValue::Unresolved(_) = value {
            return Err(LoroError::PrelimError);
        }
        Ok((PrelimValue::Value(value), None))
    }

    fn integrate(self, _txn: &mut Transaction, _container: ContainerIdx) -> Result<(), LoroError> {
        Ok(())
    }
}

impl Prelim for ContainerType {
    fn convert_value(self) -> Result<(PrelimValue, Option<Self>), LoroError> {
        Ok((PrelimValue::Container(self), Some(self)))
    }

    fn integrate(self, _txn: &mut Transaction, _container: ContainerIdx) -> Result<(), LoroError> {
        Ok(())
    }
}

impl From<LoroValue> for PrelimValue {
    fn from(value: LoroValue) -> Self {
        PrelimValue::Value(value)
    }
}

impl From<ContainerType> for PrelimValue {
    fn from(container: ContainerType) -> Self {
        PrelimValue::Container(container)
    }
}

impl From<i32> for PrelimValue {
    fn from(v: i32) -> Self {
        PrelimValue::Value(v.into())
    }
}

impl From<f64> for PrelimValue {
    fn from(v: f64) -> Self {
        PrelimValue::Value(v.into())
    }
}

#[derive(Debug)]
pub struct PrelimText(String);

impl Prelim for PrelimText {
    fn convert_value(self) -> Result<(PrelimValue, Option<Self>), LoroError> {
        Ok((PrelimValue::Container(ContainerType::Text), Some(self)))
    }

    fn integrate(
        self,
        txn: &mut Transaction,
        container_idx: ContainerIdx,
    ) -> Result<(), LoroError> {
        let container = txn.store.get_container_by_idx(&container_idx).unwrap();
        let text = Text::from_instance(container, txn.store.this_client_id);
        text.with_container(|x| x.insert(txn, 0, &self.0))
    }
}

#[derive(Debug)]
pub struct PrelimList(Vec<LoroValue>);

impl Prelim for PrelimList {
    fn convert_value(self) -> Result<(PrelimValue, Option<Self>), LoroError> {
        Ok((PrelimValue::Container(ContainerType::List), Some(self)))
    }

    fn integrate(
        self,
        txn: &mut Transaction,
        container_idx: ContainerIdx,
    ) -> Result<(), LoroError> {
        let container = txn.store.get_container_by_idx(&container_idx).unwrap();
        let list = List::from_instance(container, txn.store.this_client_id);
        list.with_container(|x| x.insert_batch(txn, 0, self.0));
        Ok(())
    }
}

#[derive(Debug)]
pub struct PrelimMap(FxHashMap<String, LoroValue>);

impl Prelim for PrelimMap {
    fn convert_value(self) -> Result<(PrelimValue, Option<Self>), LoroError> {
        Ok((PrelimValue::Container(ContainerType::Map), Some(self)))
    }

    fn integrate(
        self,
        txn: &mut Transaction,
        container_idx: ContainerIdx,
    ) -> Result<(), LoroError> {
        let container = txn.store.get_container_by_idx(&container_idx).unwrap();
        let map = Map::from_instance(container, txn.store.this_client_id);
        for (k, value) in self.0.into_iter() {
            map.with_container(|x| x.insert(txn, k.into(), value))?;
        }
        Ok(())
    }
}

impl From<String> for PrelimText {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<Vec<LoroValue>> for PrelimList {
    fn from(values: Vec<LoroValue>) -> Self {
        Self(values)
    }
}

impl From<FxHashMap<String, LoroValue>> for PrelimMap {
    fn from(value: FxHashMap<String, LoroValue>) -> Self {
        Self(value)
    }
}
