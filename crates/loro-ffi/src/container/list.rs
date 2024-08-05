use either::Either;
use loro::{
    cursor::{Cursor, Side},
    Container, ContainerTrait, LoroList as InnerLoroList, LoroResult, ValueOrContainer,
};

use crate::{ContainerID, LoroValue};

pub struct LoroList {
    handler: InnerLoroList,
}

impl LoroList {
    /// Insert a value at the given position.
    pub fn insert(&self, pos: u64, v: impl Into<LoroValue>) -> LoroResult<()> {
        self.handler.insert(pos as usize, v.into())
    }

    /// Delete values at the given position.
    #[inline]
    pub fn delete(&self, pos: usize, len: usize) -> LoroResult<()> {
        self.handler.delete(pos, len)
    }

    /// Get the value at the given position.
    #[inline]
    pub fn get(&self, index: usize) -> Option<Either<LoroValue, Container>> {
        self.handler.get(index).map(|v| match v {
            Either::Left(v) => Either::Left(v.into()),
            Either::Right(h) => Either::Right(h),
        })
    }

    /// Get the deep value of the container.
    #[inline]
    pub fn get_deep_value(&self) -> LoroValue {
        self.handler.get_deep_value().into()
    }

    /// Get the shallow value of the container.
    ///
    /// This does not convert the state of sub-containers; instead, it represents them as [LoroValue::Container].
    #[inline]
    pub fn get_value(&self) -> LoroValue {
        self.handler.get_value().into()
    }

    /// Get the ID of the container.
    #[inline]
    pub fn id(&self) -> ContainerID {
        self.handler.id().into()
    }

    /// Pop the last element of the list.
    #[inline]
    pub fn pop(&self) -> LoroResult<Option<LoroValue>> {
        self.handler.pop().map(|v| v.map(|v| v.into()))
    }

    /// Iterate over the elements of the list.
    pub fn for_each<I>(&self, f: I)
    where
        I: FnMut((usize, ValueOrContainer)),
    {
        self.handler.for_each(f)
    }

    /// Push a container to the list.
    #[inline]
    pub fn push_container<C: ContainerTrait>(&self, child: C) -> LoroResult<C> {
        self.handler.push_container(child)
    }

    #[inline]
    pub fn insert_container<C: ContainerTrait>(&self, pos: usize, child: C) -> LoroResult<C> {
        self.handler.insert_container(pos, child)
    }

    pub fn get_cursor(&self, pos: usize, side: Side) -> Option<Cursor> {
        self.handler.get_cursor(pos, side)
    }
}
