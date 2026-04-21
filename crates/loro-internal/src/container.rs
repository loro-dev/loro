//! CRDT [Container]. Each container may have different CRDT type [ContainerType].
//! Each [Op] has an associated container. It's the [Container]'s responsibility to
//! calculate the state from the [Op]s.
//!
//! Every [Container] can take a [Snapshot], which contains [crate::LoroValue] that describes the state.
//!
use crate::{arena::SharedArena, InternalString, ID};

pub mod list;
pub mod map;
pub mod richtext;
pub mod tree;
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
    ///
    // During a transaction, we may create some containers which are deleted later. And these containers also need a unique ContainerIdx.
    // So when we encode snapshot, we need to sort the containers by ContainerIdx and change the `container` of ops to the index of containers.
    // An empty store decodes the snapshot, it will create these containers in a sequence of natural numbers so that containers and ops can correspond one-to-one
    //
    // TODO: PERF: use NonZeroU32 to save memory
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
            let a = (self.0 & Self::TYPE_MASK) >> 27;
            if self.is_unknown() {
                Self::parse_unknown_type(a)
            } else {
                ContainerType::try_from_u8(a as u8).unwrap()
            }
        }

        #[allow(unused)]
        pub(crate) fn to_index(self) -> u32 {
            self.0 & Self::INDEX_MASK
        }

        pub(crate) fn from_index_and_type(index: u32, container_type: ContainerType) -> Self {
            let prefix: u32 = if matches!(container_type, ContainerType::Unknown(_)) {
                Self::unknown_to_prefix(container_type)
            } else {
                container_type.to_u8() as u32
            } << 27;

            Self(prefix | index)
        }

        pub(crate) fn is_unknown(&self) -> bool {
            self.0 >> 31 == 1
        }

        // The type_value is >>27 first, so it's 5 bits.
        // we want to get the last 4 bits. so we use 0b1111 to get the last 4 bits.
        fn parse_unknown_type(type_value: u32) -> ContainerType {
            ContainerType::Unknown((type_value & 0b1111) as u8)
        }

        // we use the top 5 bits to represent the type of the container.
        // the first bit is whether it's an unknown type.
        // So when we convert an unknown type to a prefix, we need to set the first bit to 1.
        fn unknown_to_prefix(c: ContainerType) -> u32 {
            if let ContainerType::Unknown(c) = c {
                (0b10000 | c) as u32
            } else {
                unreachable!()
            }
        }
    }
}
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

impl IntoContainerId for &str {
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
    use super::idx::ContainerIdx;
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

    #[test]
    fn container_idx_preserves_type_bits_and_index_bits() {
        let idx = ContainerIdx::from_index_and_type(42, ContainerType::Text);
        assert_eq!(idx.get_type(), ContainerType::Text);
        assert_eq!(idx.to_index(), 42);
        assert!(!idx.is_unknown());
        assert_eq!(idx.to_string(), "ContainerIdx(Text 42)");
        assert_eq!(format!("{idx:?}"), "ContainerIdx(Text 42)");

        let max_index =
            ContainerIdx::from_index_and_type(ContainerIdx::INDEX_MASK, ContainerType::MovableList);
        assert_eq!(max_index.get_type(), ContainerType::MovableList);
        assert_eq!(max_index.to_index(), ContainerIdx::INDEX_MASK);

        let unknown = ContainerIdx::from_index_and_type(7, ContainerType::Unknown(13));
        assert!(unknown.is_unknown());
        assert_eq!(unknown.get_type(), ContainerType::Unknown(13));
        assert_eq!(unknown.to_index(), 7);
        assert_eq!(unknown.to_string(), "ContainerIdx(Unknown(13) 7)");
    }

    #[test]
    fn container_id_raw_reapplies_type_for_root_and_normal_ids() {
        let root = ContainerIdRaw::from("notes").with_type(ContainerType::Map);
        assert_eq!(root, ContainerID::new_root("notes", ContainerType::Map));

        let normal_id = ID::new(7, 11);
        let normal = ContainerIdRaw::from(ContainerID::new_normal(normal_id, ContainerType::List))
            .with_type(ContainerType::Text);
        assert_eq!(
            normal,
            ContainerID::new_normal(normal_id, ContainerType::Text)
        );

        let root_again = ContainerIdRaw::from(&root).with_type(ContainerType::Tree);
        assert_eq!(
            root_again,
            ContainerID::new_root("notes", ContainerType::Tree)
        );
    }

    #[test]
    fn into_container_id_uses_root_names_ids_and_arena_indexes() {
        let arena = SharedArena::new();
        assert_eq!(
            "workspace".into_container_id(&arena, ContainerType::Map),
            ContainerID::new_root("workspace", ContainerType::Map)
        );
        assert_eq!(
            String::from("text").into_container_id(&arena, ContainerType::Text),
            ContainerID::new_root("text", ContainerType::Text)
        );

        let normal = ContainerID::new_normal(ID::new(1, 2), ContainerType::List);
        assert_eq!(
            (&normal).into_container_id(&arena, ContainerType::Map),
            normal
        );
        assert_eq!(
            normal
                .clone()
                .into_container_id(&arena, ContainerType::Text),
            normal
        );

        let idx = arena.register_container(&normal);
        assert_eq!(idx.into_container_id(&arena, ContainerType::List), normal);
        assert_eq!(
            (&idx).into_container_id(&arena, ContainerType::List),
            normal
        );
    }

    #[test]
    #[should_panic(expected = "assertion `left == right` failed")]
    fn into_container_id_rejects_mismatched_index_type() {
        let arena = SharedArena::new();
        let id = ContainerID::new_root("items", ContainerType::List);
        let idx = arena.register_container(&id);

        let _ = idx.into_container_id(&arena, ContainerType::Map);
    }
}
