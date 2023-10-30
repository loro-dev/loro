use std::{fmt::Display, sync::Arc};

use arbitrary::Arbitrary;
use enum_as_inner::EnumAsInner;

use fxhash::FxHashMap;
use serde::{Deserialize, Serialize};
mod error;
mod id;
mod span;
mod value;

pub use error::{LoroError, LoroResult, LoroTreeError};
pub use span::*;
pub use value::LoroValue;

use zerovec::ule::AsULE;
pub type PeerID = u64;
pub type Counter = i32;
pub type Lamport = u32;

#[derive(PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
pub struct ID {
    pub peer: PeerID,
    pub counter: Counter,
}

/// [ContainerID] includes the Op's [ID] and the type. So it's impossible to have
/// the same [ContainerID] with conflict [ContainerType].
///
/// This structure is really cheap to clone.
///
/// String representation:
///
/// - Root Container: `/<name>:<type>`
/// - Normal Container: `<counter>@<client>:<type>`
///
/// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[derive(Hash, PartialEq, Eq, Debug, Clone, Serialize, Deserialize, EnumAsInner)]
pub enum ContainerID {
    /// Root container does not need an op to create. It can be created implicitly.
    Root {
        name: InternalString,
        container_type: ContainerType,
    },
    Normal {
        peer: PeerID,
        counter: Counter,
        container_type: ContainerType,
    },
}

pub type InternalString = string_cache::DefaultAtom;
// TODO: add non_exausted
// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[derive(
    Arbitrary, Debug, PartialEq, Eq, Hash, Clone, Copy, PartialOrd, Ord, Serialize, Deserialize,
)]
pub enum ContainerType {
    /// See [`crate::text::TextContent`]
    Text,
    Map,
    List,
    Tree,
    // TODO: Users can define their own container types.
    // Custom(u16),
}

impl AsULE for ContainerType {
    type ULE = u8;

    fn to_unaligned(self) -> Self::ULE {
        match self {
            ContainerType::Map => 1,
            ContainerType::List => 2,
            ContainerType::Text => 3,
            ContainerType::Tree => 4,
        }
    }

    fn from_unaligned(unaligned: Self::ULE) -> Self {
        match unaligned {
            1 => ContainerType::Map,
            2 => ContainerType::List,
            3 => ContainerType::Text,
            4 => ContainerType::Tree,
            _ => unreachable!(),
        }
    }
}

impl ContainerType {
    pub fn default_value(&self) -> LoroValue {
        match self {
            ContainerType::Map => LoroValue::Map(Arc::new(Default::default())),
            ContainerType::List => LoroValue::List(Arc::new(Default::default())),
            ContainerType::Text => LoroValue::String(Arc::new(Default::default())),
            ContainerType::Tree => LoroValue::List(Arc::new(Default::default())),
        }
    }

    pub fn to_u8(self) -> u8 {
        match self {
            ContainerType::Map => 1,
            ContainerType::List => 2,
            ContainerType::Text => 3,
            ContainerType::Tree => 4,
        }
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => ContainerType::Map,
            2 => ContainerType::List,
            3 => ContainerType::Text,
            4 => ContainerType::Tree,
            _ => unreachable!(),
        }
    }
}

// a weird dependency in Prelim in loro_internal need this convertion to work.
// this can be removed after the Prelim is removed.
impl From<ContainerType> for LoroValue {
    fn from(value: ContainerType) -> Self {
        LoroValue::Container(ContainerID::Normal {
            peer: 0,
            counter: 0,
            container_type: value,
        })
    }
}

pub type IdSpanVector = fxhash::FxHashMap<PeerID, CounterSpan>;

mod container {
    use super::*;

    impl Display for ContainerType {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self {
                ContainerType::Map => "Map",
                ContainerType::List => "List",
                ContainerType::Text => "Text",
                ContainerType::Tree => "Tree",
            })
        }
    }

    impl Display for ContainerID {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                ContainerID::Root {
                    name,
                    container_type,
                } => f.write_fmt(format_args!("/{}:{}", name, container_type))?,
                ContainerID::Normal {
                    peer,
                    counter,
                    container_type,
                } => f.write_fmt(format_args!(
                    "{}:{}",
                    ID::new(*peer, *counter),
                    container_type
                ))?,
            };
            Ok(())
        }
    }

    impl TryFrom<&str> for ContainerID {
        type Error = ();

        fn try_from(value: &str) -> Result<Self, Self::Error> {
            let mut parts = value.split(':');
            let id = parts.next().ok_or(())?;
            let container_type = parts.next().ok_or(())?;
            let container_type = ContainerType::try_from(container_type).map_err(|_| ())?;
            if let Some(id) = id.strip_prefix('/') {
                Ok(ContainerID::Root {
                    name: id.into(),
                    container_type,
                })
            } else {
                let mut parts = id.split('@');
                let counter = parts.next().ok_or(())?.parse().map_err(|_| ())?;
                let client = parts.next().ok_or(())?.parse().map_err(|_| ())?;
                Ok(ContainerID::Normal {
                    counter,
                    peer: client,
                    container_type,
                })
            }
        }
    }

    impl ContainerID {
        #[inline]
        pub fn new_normal(id: ID, container_type: ContainerType) -> Self {
            ContainerID::Normal {
                peer: id.peer,
                counter: id.counter,
                container_type,
            }
        }

        #[inline]
        pub fn new_root(name: &str, container_type: ContainerType) -> Self {
            ContainerID::Root {
                name: name.into(),
                container_type,
            }
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

    impl TryFrom<&str> for ContainerType {
        type Error = LoroError;

        fn try_from(value: &str) -> Result<Self, Self::Error> {
            match value {
                "Map" => Ok(ContainerType::Map),
                "List" => Ok(ContainerType::List),
                "Text" => Ok(ContainerType::Text),
                "Tree" => Ok(ContainerType::Tree),
                _ => Err(LoroError::DecodeError(
                    ("Unknown container type".to_string() + value).into(),
                )),
            }
        }
    }
}

/// In movable tree, we use a specific [`TreeID`] to represent the root of **ALL** non-existent tree nodes.
///
/// When we create some tree node and then we checkout the previous vision, we need to delete it from the state.
/// If the parent of node is [`UNEXIST_TREE_ROOT`], we could infer this node is first created and delete it from the state directly,
/// instead of moving it to the [`DELETED_TREE_ROOT`].
///
/// This root only can be old parent of node.
pub const UNEXIST_TREE_ROOT: Option<TreeID> = Some(TreeID {
    peer: PeerID::MAX,
    counter: Counter::MAX - 1,
});

/// In movable tree, we use a specific [`TreeID`] to represent the root of **ALL** deleted tree node.
///
/// Deletion operation is equivalent to move target tree node to [`DELETED_TREE_ROOT`].
pub const DELETED_TREE_ROOT: Option<TreeID> = Some(TreeID {
    peer: PeerID::MAX,
    counter: Counter::MAX,
});

/// Each node of movable tree has a unique [`TreeID`] generated by Loro.
///
/// To further represent the metadata (a MapContainer) associated with each node,
/// we also use [`TreeID`] as [`ID`] portion of [`ContainerID`].
/// This not only allows for convenient association of metadata with each node,
/// but also ensures the uniqueness of the MapContainer.
///
/// Special ID:
/// - [`DELETED_TREE_ROOT`]: the root of all deleted nodes. To get it by [`TreeID::delete_root()`]
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TreeID {
    pub peer: PeerID,
    pub counter: Counter,
}

impl TreeID {
    /// return [`DELETED_TREE_ROOT`]
    pub const fn delete_root() -> Option<Self> {
        DELETED_TREE_ROOT
    }

    /// return `true` if the `TreeID` is deleted root
    pub fn is_deleted_root(target: Option<TreeID>) -> bool {
        target == DELETED_TREE_ROOT
    }

    pub const fn unexist_root() -> Option<Self> {
        UNEXIST_TREE_ROOT
    }

    /// return `true` if the `TreeID` is non-existent root
    pub fn is_unexist_root(target: Option<TreeID>) -> bool {
        target == UNEXIST_TREE_ROOT
    }

    pub fn from_id(id: ID) -> Self {
        Self {
            peer: id.peer,
            counter: id.counter,
        }
    }

    pub fn id(&self) -> ID {
        ID {
            peer: self.peer,
            counter: self.counter,
        }
    }

    pub fn associated_meta_container(&self) -> ContainerID {
        ContainerID::new_normal(self.id(), ContainerType::Map)
    }
}

impl Display for TreeID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.id().fmt(f)
    }
}

impl TryFrom<&str> for TreeID {
    type Error = ();
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let mut parts = value.split('@');
        let counter = parts.next().ok_or(())?.parse().map_err(|_| ())?;
        let peer = parts.next().ok_or(())?.parse().map_err(|_| ())?;
        Ok(TreeID { peer, counter })
    }
}

#[cfg(feature = "wasm")]
pub mod wasm {
    use crate::TreeID;
    use wasm_bindgen::JsValue;
    impl From<TreeID> for JsValue {
        fn from(value: TreeID) -> Self {
            JsValue::from_str(&format!("{}", value.id()))
        }
    }

    impl TryFrom<JsValue> for TreeID {
        type Error = ();
        fn try_from(value: JsValue) -> Result<Self, Self::Error> {
            let id = value.as_string().unwrap();
            let mut parts = id.split('@');
            let counter = parts.next().ok_or(())?.parse().map_err(|_| ())?;
            let peer = parts.next().ok_or(())?.parse().map_err(|_| ())?;
            Ok(TreeID { peer, counter })
        }
    }
}
