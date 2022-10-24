use crate::{InsertContentTrait, ID};
use rle::{HasLength, Mergable, Sliceable};
use serde::Serialize;

#[derive(Debug, Clone)]
pub(crate) enum Slot {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum ContainerType {
    /// See [`crate::text::TextContent`]
    Text,
    Map,
    List,
    /// Users can define their own container types.
    Custom(u16),
}

/// Container is a special kind of op content. Each container has its own CRDT implementation.
/// Each [Op] must be associated with a container.
///
#[derive(Debug, Clone)]
pub struct ContainerContent {
    parent: ID,
    container_type: ContainerType,
}

impl HasLength for ContainerContent {
    fn content_len(&self) -> usize {
        1
    }
}

impl Mergable for ContainerContent {
    fn is_mergable(&self, _: &Self, _: &()) -> bool
    where
        Self: Sized,
    {
        false
    }

    fn merge(&mut self, _: &Self, _: &())
    where
        Self: Sized,
    {
        unreachable!()
    }
}

impl Sliceable for ContainerContent {
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(from == 0 && to == 1);
        self.clone()
    }
}

impl InsertContentTrait for ContainerContent {
    fn id(&self) -> crate::ContentType {
        crate::ContentType::Container
    }
}
