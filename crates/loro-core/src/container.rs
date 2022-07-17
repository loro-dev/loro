use crate::{
    op::OpProxy, snapshot::Snapshot, version::VersionVector, InsertContent, InternalString,
    LogStore, Op, SmString, ID,
};
use rle::{HasLength, Mergable, Sliceable};
use std::alloc::Layout;

mod container_content;
pub mod map;
pub mod text;
pub use container_content::*;

pub trait Container {
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
