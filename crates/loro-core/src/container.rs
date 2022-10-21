//! CRDT [Container]. Each container may have different CRDT type [ContainerType].
//! Each [Op] has an associated container. It's the [Container]'s responsibility to
//! calculate the state from the [Op]s.
//!
//! Every [Container] can take a [Snapshot], which contains [crate::LoroValue] that describes the state.
//!
use crate::{
    op::Op, span::IdSpan, version::VersionVector, InternalString, LogStore, LoroValue, ID,
};

use serde::Serialize;
use std::{any::Any, fmt::Debug};

mod container_content;
pub mod manager;

pub mod list;
pub mod map;
pub mod text;
pub use container_content::*;

pub trait Container: Debug + Any + Unpin {
    fn id(&self) -> &ContainerID;
    fn type_(&self) -> ContainerType;
    /// NOTE: this method expect that [LogStore] has store the Change
    fn apply(&mut self, id_span: IdSpan, log: &LogStore);
    fn checkout_version(&mut self, vv: &VersionVector);
    fn get_value(&mut self) -> &LoroValue;
    // TODO: need a custom serializer
    // fn serialize(&self) -> Vec<u8>;

    /// convert an op to export format. for example [ListSlice] should be convert to str before export
    fn to_export(&self, op: &mut Op);
}

/// it's really cheap to clone
#[derive(Hash, PartialEq, Eq, Debug, Clone, Serialize)]
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
