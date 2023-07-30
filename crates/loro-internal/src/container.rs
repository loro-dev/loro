//! CRDT [Container]. Each container may have different CRDT type [ContainerType].
//! Each [Op] has an associated container. It's the [Container]'s responsibility to
//! calculate the state from the [Op]s.
//!
//! Every [Container] can take a [Snapshot], which contains [crate::LoroValue] that describes the state.
//!
use crate::{arena::SharedArena, InternalString, ID};

pub mod registry;

pub mod list;
pub mod map;
pub mod text;

use registry::ContainerIdx;

pub use loro_common::ContainerType;

pub use loro_common::ContainerID;

use crate::{event::Diff, VersionVector};

use super::oplog::OpLog;

pub trait Container {
    fn diff(&self, log: &OpLog, before: &VersionVector, after: &VersionVector) -> Vec<Diff>;
}

pub enum ContainerIdRaw {
    Root { name: InternalString },
    Normal { id: ID },
}

pub trait IntoContainerId {
    fn into_container_id(self, arena: &SharedArena, kind: ContainerType) -> ContainerID;
}

impl IntoContainerId for String {
    fn into_container_id(self, _arena: &SharedArena, kind: ContainerType) -> ContainerID {
        ContainerID::Root {
            name: InternalString::from(self.as_str()),
            container_type: kind,
        }
    }
}

impl<'a> IntoContainerId for &'a str {
    fn into_container_id(self, _arena: &SharedArena, kind: ContainerType) -> ContainerID {
        ContainerID::Root {
            name: InternalString::from(self),
            container_type: kind,
        }
    }
}

impl IntoContainerId for ContainerID {
    fn into_container_id(self, _arena: &SharedArena, _kind: ContainerType) -> ContainerID {
        self
    }
}

impl IntoContainerId for ContainerIdx {
    fn into_container_id(self, arena: &SharedArena, kind: ContainerType) -> ContainerID {
        assert_eq!(self.get_type(), kind);
        arena.get_container_id(self).unwrap()
    }
}

impl From<String> for ContainerIdRaw {
    fn from(value: String) -> Self {
        ContainerIdRaw::Root { name: value.into() }
    }
}

impl<'a> From<&'a str> for ContainerIdRaw {
    fn from(value: &'a str) -> Self {
        ContainerIdRaw::Root { name: value.into() }
    }
}

impl From<&ContainerID> for ContainerIdRaw {
    fn from(id: &ContainerID) -> Self {
        match id {
            ContainerID::Root { name, .. } => ContainerIdRaw::Root { name: name.clone() },
            ContainerID::Normal { peer, counter, .. } => ContainerIdRaw::Normal {
                id: ID::new(*peer, *counter),
            },
        }
    }
}

impl From<ContainerID> for ContainerIdRaw {
    fn from(id: ContainerID) -> Self {
        match id {
            ContainerID::Root { name, .. } => ContainerIdRaw::Root { name },
            ContainerID::Normal { peer, counter, .. } => ContainerIdRaw::Normal {
                id: ID::new(peer, counter),
            },
        }
    }
}

impl ContainerIdRaw {
    pub fn with_type(self, container_type: ContainerType) -> ContainerID {
        match self {
            ContainerIdRaw::Root { name } => ContainerID::Root {
                name,
                container_type,
            },
            ContainerIdRaw::Normal { id } => ContainerID::Normal {
                peer: id.peer,
                counter: id.counter,
                container_type,
            },
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn container_id_convert() {
        let container_id = ContainerID::new_normal(ID::new(12, 12), ContainerType::List);
        let s = container_id.to_string();
        assert_eq!(s, "12@12:List");
        let actual = ContainerID::try_from(s.as_str()).unwrap();
        assert_eq!(actual, container_id);

        let container_id = ContainerID::new_root("123", ContainerType::Map);
        let s = container_id.to_string();
        assert_eq!(s, "/123:Map");
        let actual = ContainerID::try_from(s.as_str()).unwrap();
        assert_eq!(actual, container_id);

        let container_id = ContainerID::new_root("kkk", ContainerType::Text);
        let s = container_id.to_string();
        assert_eq!(s, "/kkk:Text");
        let actual = ContainerID::try_from(s.as_str()).unwrap();
        assert_eq!(actual, container_id);
    }
}
