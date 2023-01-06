use std::sync::{Mutex, Weak};

use enum_as_inner::EnumAsInner;

use crate::{container::registry::ContainerInstance, context::Context, ContainerType, LoroValue};

/// Prelim is a value that is not yet integrated into the Loro.
pub trait Prelim: Sized {
    /// Convert the value into a [`PrelimValue`].
    /// If the value is preliminary(container-like), return [`PrelimValue::Container`] and `Some(self)`
    /// that means the value needs to be integrated into the Loro by creating another container.
    ///
    /// If the value is not preliminary, return [`PrelimValue::Value`] and `None`. The value will be insert into the container of Loro directly.
    fn convert_value(self) -> (PrelimValue, Option<Self>);

    /// How to integrate the value into the Loro.
    fn integrate<C: Context>(self, ctx: &C, container: Weak<Mutex<ContainerInstance>>);
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
    fn convert_value(self) -> (PrelimValue, Option<Self>) {
        // TODO: check LoroValue::Unresolved
        let value: LoroValue = self.into();
        (PrelimValue::Value(value), None)
    }

    fn integrate<C: Context>(self, _ctx: &C, _container: Weak<Mutex<ContainerInstance>>) {}
}

impl Prelim for ContainerType {
    fn convert_value(self) -> (PrelimValue, Option<Self>) {
        (PrelimValue::Container(self), Some(self))
    }

    fn integrate<C: Context>(self, _ctx: &C, _container: Weak<Mutex<ContainerInstance>>) {}
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
