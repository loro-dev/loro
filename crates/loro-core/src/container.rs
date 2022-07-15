use rle::{HasLength, Mergable, Sliceable};
use smartstring::{LazyCompact, SmartString};

use crate::{InsertContent, ID};

/// Container is a special kind of op content. Each container has its own CRDT implementation.
/// Each [Op] must be associated with a container.
///
#[derive(Debug, Clone)]
pub struct Container {
    parent: Option<ID>,
    parent_slot: SmartString<LazyCompact>,
}

impl HasLength for Container {
    fn len(&self) -> usize {
        1
    }
}

impl Mergable for Container {
    fn is_mergable(&self, other: &Self, conf: &()) -> bool
    where
        Self: Sized,
    {
        false
    }

    fn merge(&mut self, other: &Self, conf: &())
    where
        Self: Sized,
    {
        unreachable!()
    }
}

impl Sliceable for Container {
    fn slice(&self, from: usize, to: usize) -> Self {
        assert!(from == 0 && to == 1);
        self.clone()
    }
}

impl InsertContent for Container {
    fn id(&self) -> crate::ContentTypeID {
        crate::ContentTypeID::Container
    }
}
