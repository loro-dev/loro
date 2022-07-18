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
use std::{alloc::Layout, fmt::Debug};

mod container_content;
mod manager;

pub mod map;
pub mod text;
pub use container_content::*;
pub use manager::*;

pub trait Container: Debug {
    fn id(&self) -> &ContainerID;
    fn type_id(&self) -> ContainerType;
    fn apply(&mut self, op: &OpProxy);
    fn snapshot(&mut self) -> &Snapshot;
    fn checkout_version(&mut self, vv: &VersionVector, log: &LogStore);
}

#[derive(Hash, PartialEq, Eq, Debug, Clone)]
pub enum ContainerID {
    /// Root container does not need a insert op to create. It can be created implicitly.
    Root {
        name: InternalString,
        container_type: ContainerType,
    },
    Normal(ID),
}
