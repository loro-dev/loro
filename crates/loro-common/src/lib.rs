use std::{fmt::Display, sync::Arc};

use arbitrary::Arbitrary;
use enum_as_inner::EnumAsInner;

use nonmax::NonMaxI32;
use serde::{Deserialize, Serialize};
mod error;
mod id;
mod internal_string;
mod macros;
mod span;
mod value;

pub use error::{LoroError, LoroResult, LoroTreeError};
#[doc(hidden)]
pub use fxhash::FxHashMap;
pub use internal_string::InternalString;
pub use span::*;
pub use value::{to_value, LoroValue};

/// Unique id for each peer. It's a random u64 by default.
pub type PeerID = u64;
/// If it's the nth Op of a peer, the counter will be n.
pub type Counter = i32;
/// It's the [Lamport clock](https://en.wikipedia.org/wiki/Lamport_timestamp)
pub type Lamport = u32;

/// It's the unique ID of an Op represented by [PeerID] and [Counter].
#[derive(PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
pub struct ID {
    pub peer: PeerID,
    pub counter: Counter,
}

/// It's the unique ID of an Op represented by [PeerID] and [Counter].
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct CompactId {
    pub peer: PeerID,
    pub counter: NonMaxI32,
}

impl CompactId {
    pub fn new(peer: PeerID, counter: Counter) -> Self {
        Self {
            peer,
            counter: NonMaxI32::new(counter).unwrap(),
        }
    }

    pub fn to_id(&self) -> ID {
        ID {
            peer: self.peer,
            counter: self.counter.get(),
        }
    }

    pub fn inc(&self, start: i32) -> CompactId {
        Self {
            peer: self.peer,
            counter: NonMaxI32::new(start + self.counter.get()).unwrap(),
        }
    }
}

impl TryFrom<ID> for CompactId {
    type Error = ID;

    fn try_from(id: ID) -> Result<Self, ID> {
        if id.counter == i32::MAX {
            return Err(id);
        }

        Ok(Self::new(id.peer, id.counter))
    }
}

/// It's the unique ID of an Op represented by [PeerID] and [Lamport] clock.
/// It's used to define the total order of Ops.
#[derive(PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize, PartialOrd, Ord)]
pub struct IdLp {
    pub lamport: Lamport,
    pub peer: PeerID,
}

/// It's the unique ID of an Op represented by [PeerID], [Lamport] clock and [Counter].
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
pub struct IdFull {
    pub peer: PeerID,
    pub lamport: Lamport,
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

    pub fn try_from_u8(v: u8) -> LoroResult<Self> {
        match v {
            1 => Ok(ContainerType::Map),
            2 => Ok(ContainerType::List),
            3 => Ok(ContainerType::Text),
            4 => Ok(ContainerType::Tree),
            _ => Err(LoroError::DecodeError(
                format!("Unknown container type {v}").into_boxed_str(),
            )),
        }
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
                } => f.write_fmt(format_args!("cid:root-{}:{}", name, container_type))?,
                ContainerID::Normal {
                    peer,
                    counter,
                    container_type,
                } => f.write_fmt(format_args!(
                    "cid:{}:{}",
                    ID::new(*peer, *counter),
                    container_type
                ))?,
            };
            Ok(())
        }
    }

    impl TryFrom<&str> for ContainerID {
        type Error = ();

        fn try_from(mut s: &str) -> Result<Self, Self::Error> {
            if !s.starts_with("cid:") {
                return Err(());
            }

            s = &s[4..];
            if s.starts_with("root-") {
                // root container
                s = &s[5..];
                let split = s.rfind(':').ok_or(())?;
                if split == 0 {
                    return Err(());
                }
                let kind = ContainerType::try_from(&s[split + 1..]).map_err(|_| ())?;
                let name = &s[..split];
                Ok(ContainerID::Root {
                    name: name.into(),
                    container_type: kind,
                })
            } else {
                let mut iter = s.split(':');
                let id = iter.next().ok_or(())?;
                let kind = iter.next().ok_or(())?;
                if iter.next().is_some() {
                    return Err(());
                }

                let id = ID::try_from(id).map_err(|_| ())?;
                let kind = ContainerType::try_from(kind).map_err(|_| ())?;
                Ok(ContainerID::Normal {
                    peer: id.peer,
                    counter: id.counter,
                    container_type: kind,
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

/// In movable tree, we use a specific [`TreeID`] to represent the root of **ALL** deleted tree node.
///
/// Deletion operation is equivalent to move target tree node to [`DELETED_TREE_ROOT`].
pub const DELETED_TREE_ROOT: TreeID = TreeID {
    peer: PeerID::MAX,
    counter: Counter::MAX,
};

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
    #[inline(always)]
    pub fn new(peer: PeerID, counter: Counter) -> Self {
        Self { peer, counter }
    }

    /// return [`DELETED_TREE_ROOT`]
    pub const fn delete_root() -> Self {
        DELETED_TREE_ROOT
    }

    /// return `true` if the `TreeID` is deleted root
    pub fn is_deleted_root(target: &TreeID) -> bool {
        target == &DELETED_TREE_ROOT
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
    type Error = LoroError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let id = ID::try_from(value)?;
        Ok(TreeID {
            peer: id.peer,
            counter: id.counter,
        })
    }
}

#[cfg(feature = "wasm")]
pub mod wasm {
    use crate::{LoroError, TreeID};
    use wasm_bindgen::JsValue;
    impl From<TreeID> for JsValue {
        fn from(value: TreeID) -> Self {
            JsValue::from_str(&format!("{}", value.id()))
        }
    }

    impl TryFrom<JsValue> for TreeID {
        type Error = LoroError;
        fn try_from(value: JsValue) -> Result<Self, Self::Error> {
            let id = value.as_string().unwrap();
            TreeID::try_from(id.as_str())
        }
    }
}

#[cfg(test)]
mod test {
    use crate::ContainerID;

    #[test]
    fn test_container_id_convert_to_and_from_str() {
        let id = ContainerID::Root {
            name: "name".into(),
            container_type: crate::ContainerType::Map,
        };
        let id_str = id.to_string();
        assert_eq!(id_str.as_str(), "cid:root-name:Map");
        assert_eq!(ContainerID::try_from(id_str.as_str()).unwrap(), id);

        let id = ContainerID::Normal {
            counter: 10,
            peer: 255,
            container_type: crate::ContainerType::Map,
        };
        let id_str = id.to_string();
        assert_eq!(id_str.as_str(), "cid:10@255:Map");
        assert_eq!(ContainerID::try_from(id_str.as_str()).unwrap(), id);

        let id = ContainerID::try_from("cid:root-a:b:c:Tree").unwrap();
        assert_eq!(
            id,
            ContainerID::new_root("a:b:c", crate::ContainerType::Tree)
        );
    }

    #[test]
    fn test_convert_invalid_container_id_str() {
        assert!(ContainerID::try_from("cid:root-:Map").is_err());
        assert!(ContainerID::try_from("cid:0@:Map").is_err());
        assert!(ContainerID::try_from("cid:@:Map").is_err());
        assert!(ContainerID::try_from("cid:x@0:Map").is_err());
        assert!(ContainerID::try_from("id:0@0:Map").is_err());
    }
}
