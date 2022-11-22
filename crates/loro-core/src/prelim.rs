use std::sync::{Arc, Mutex};

use crate::{container::registry::ContainerInstance, context::Context, ContainerType, LoroValue};

pub trait Prelim: Sized {
    fn container_type(&self) -> Option<ContainerType>;
    fn into_loro_value(self) -> LoroValue;
    fn integrate<C: Context>(self, ctx: &C, container_id: &Arc<Mutex<ContainerInstance>>);
}

impl<T> Prelim for T
where
    T: Into<LoroValue>,
{
    fn container_type(&self) -> Option<ContainerType> {
        None
    }

    fn into_loro_value(self) -> LoroValue {
        self.into()
    }

    fn integrate<C: Context>(self, _ctx: &C, _container: &Arc<Mutex<ContainerInstance>>) {}
}

impl Prelim for ContainerType {
    fn container_type(&self) -> Option<ContainerType> {
        Some(*self)
    }

    fn into_loro_value(self) -> LoroValue {
        unreachable!()
    }

    fn integrate<C: Context>(self, _ctx: &C, _container: &Arc<Mutex<ContainerInstance>>) {}
}
