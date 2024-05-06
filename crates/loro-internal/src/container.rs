//! CRDT [Container]. Each container may have different CRDT type [ContainerType].
//! Each [Op] has an associated container. It's the [Container]'s responsibility to
//! calculate the state from the [Op]s.
//!
//! Every [Container] can take a [Snapshot], which contains [crate::LoroValue] that describes the state.
//!
use crate::{arena::SharedArena, InternalString, ID};

pub mod idx {
    use super::super::ContainerType;

    /// Inner representation for ContainerID.
    /// It contains the unique index for the container and the type of the container.
    /// It uses top 4 bits to represent the type of the container.
    ///
    /// It's only used inside this crate and should not be exposed to the user.
    ///
    /// TODO: make this type private in this crate only
    ///
    // During a transaction, we may create some containers which are deleted later. And these containers also need a unique ContainerIdx.
    // So when we encode snapshot, we need to sort the containers by ContainerIdx and change the `container` of ops to the index of containers.
    // An empty store decodes the snapshot, it will create these containers in a sequence of natural numbers so that containers and ops can correspond one-to-one
    #[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
    pub struct ContainerIdx(u32);

    impl std::fmt::Debug for ContainerIdx {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "ContainerIdx({} {})", self.get_type(), self.to_index())
        }
    }

    impl std::fmt::Display for ContainerIdx {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "ContainerIdx({} {})", self.get_type(), self.to_index())
        }
    }

    impl ContainerIdx {
        pub(crate) const TYPE_MASK: u32 = 0b11111 << 27;
        pub(crate) const INDEX_MASK: u32 = !Self::TYPE_MASK;

        #[allow(unused)]
        pub(crate) fn get_type(self) -> ContainerType {
            match (self.0 & Self::TYPE_MASK) >> 27 {
                0 => ContainerType::Map,
                1 => ContainerType::List,
                2 => ContainerType::Text,
                3 => ContainerType::Tree,
                4 => ContainerType::MovableList,
                a if self.is_unknown() => ContainerType::Unknown((a << 1 >> 28) as u8),
                _ => unreachable!(),
            }
        }

        #[allow(unused)]
        pub(crate) fn to_index(self) -> u32 {
            self.0 & Self::INDEX_MASK
        }

        pub(crate) fn from_index_and_type(index: u32, container_type: ContainerType) -> Self {
            let prefix: u32 = match container_type {
                ContainerType::Map => 0,
                ContainerType::List => 1,
                ContainerType::Text => 2,
                ContainerType::Tree => 3,
                ContainerType::MovableList => 4,
                ContainerType::Unknown(c) => (0b10000 | c) as u32,
            } << 27;

            Self(prefix | index)
        }

        pub(crate) fn is_unknown(&self) -> bool {
            self.0 >> 31 == 1
        }
    }
}

pub mod list;
pub mod map;
pub mod richtext;
pub mod tree;

use idx::ContainerIdx;

pub use loro_common::ContainerType;

pub use loro_common::ContainerID;

#[derive(Debug)]
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

impl IntoContainerId for &ContainerID {
    fn into_container_id(self, _arena: &SharedArena, _kind: ContainerType) -> ContainerID {
        self.clone()
    }
}

impl IntoContainerId for ContainerIdx {
    fn into_container_id(self, arena: &SharedArena, kind: ContainerType) -> ContainerID {
        assert_eq!(self.get_type(), kind);
        arena.get_container_id(self).unwrap()
    }
}

impl IntoContainerId for &ContainerIdx {
    fn into_container_id(self, arena: &SharedArena, kind: ContainerType) -> ContainerID {
        assert_eq!(self.get_type(), kind);
        arena.get_container_id(*self).unwrap()
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
        assert_eq!(s, "cid:12@12:List");
        let actual = ContainerID::try_from(s.as_str()).unwrap();
        assert_eq!(actual, container_id);

        let container_id = ContainerID::new_root("123", ContainerType::Map);
        let s = container_id.to_string();
        assert_eq!(s, "cid:root-123:Map");
        let actual = ContainerID::try_from(s.as_str()).unwrap();
        assert_eq!(actual, container_id);

        let container_id = ContainerID::new_root("kkk", ContainerType::Text);
        let s = container_id.to_string();
        assert_eq!(s, "cid:root-kkk:Text");
        let actual = ContainerID::try_from(s.as_str()).unwrap();
        assert_eq!(actual, container_id);
    }
}
