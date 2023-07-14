use std::fmt::Display;

use arbitrary::Arbitrary;
use serde::{Deserialize, Serialize};
mod error;
mod id;
mod span;
mod value;

pub use error::LoroError;
pub use span::*;
pub use value::LoroValue;
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
#[derive(Hash, PartialEq, Eq, Debug, Clone, Serialize, Deserialize)]
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
// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[derive(Arbitrary, Debug, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
pub enum ContainerType {
    /// See [`crate::text::TextContent`]
    Text,
    Map,
    List,
    // TODO: Users can define their own container types.
    // Custom(u16),
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
                ContainerType::Text => "Text",
                ContainerType::Map => "Map",
                ContainerType::List => "List",
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

    impl TryFrom<&str> for ContainerType {
        type Error = LoroError;

        fn try_from(value: &str) -> Result<Self, Self::Error> {
            match value {
                "Text" => Ok(ContainerType::Text),
                "Map" => Ok(ContainerType::Map),
                "List" => Ok(ContainerType::List),
                _ => Err(LoroError::DecodeError(
                    ("Unknown container type".to_string() + value).into(),
                )),
            }
        }
    }
}
