//! CRDT [Container]. Each container may have different CRDT type [ContainerType].
//! Each [Op] has an associated container. It's the [Container]'s responsibility to
//! calculate the state from the [Op]s.
//!
//! Every [Container] can take a [Snapshot], which contains [crate::LoroValue] that describes the state.
//!
use crate::{
    op::OpProxy, snapshot::Snapshot, version::VersionVector, InsertContent, InternalString,
    LogStore, Op, SmString, ID,
};
use rle::{HasLength, Mergable, Sliceable};
use std::{
    alloc::Layout,
    any::{self, Any, TypeId},
    fmt::Debug,
    pin::Pin,
};

mod container_content;
mod manager;

pub mod map;
pub mod text;
pub use container_content::*;
pub use manager::*;

pub trait Container: Debug + Any + Unpin {
    fn id(&self) -> &ContainerID;
    fn container_type(&self) -> ContainerType;
    fn apply(&mut self, op: &OpProxy);
    fn snapshot(&mut self) -> Snapshot;
    fn checkout_version(&mut self, vv: &VersionVector, log: &LogStore);
}

pub(crate) trait Cast<T> {
    fn cast(&self) -> &T;
    fn cast_mut(&mut self) -> &mut T;
}

impl<T: Any> Cast<T> for dyn Container {
    fn cast(&self) -> &T {
        let t = self as *const dyn Container as *const T;
        unsafe { &*t }
    }

    fn cast_mut(&mut self) -> &mut T {
        let t = self as *mut dyn Container as *mut T;
        unsafe { &mut *t }
    }
}

#[derive(Hash, PartialEq, Eq, Debug, Clone)]
pub enum ContainerID {
    /// Root container does not need a insert op to create. It can be created implicitly.
    Root {
        name: InternalString,
        container_type: ContainerType,
    },
    Normal {
        id: ID,
        container_type: ContainerType,
    },
}

impl ContainerID {
    #[inline]
    pub fn new_normal(id: ID, container_type: ContainerType) -> Self {
        ContainerID::Normal { id, container_type }
    }

    #[inline]
    pub fn new_root(name: InternalString, container_type: ContainerType) -> Self {
        ContainerID::Root {
            name,
            container_type,
        }
    }

    #[inline]
    pub fn is_root(&self) -> bool {
        matches!(self, ContainerID::Root { .. })
    }

    #[inline]
    pub fn is_normal(&self) -> bool {
        matches!(self, ContainerID::Normal { .. })
    }

    #[inline]
    pub fn name(&self) -> &InternalString {
        match self {
            ContainerID::Root { name, .. } => name,
            ContainerID::Normal { .. } => unreachable!(),
        }
    }

    #[inline]
    pub fn container_type(&self) -> ContainerType {
        match self {
            ContainerID::Root { container_type, .. } => *container_type,
            ContainerID::Normal { container_type, .. } => *container_type,
        }
    }
}
