//! CRDT [Container]. Each container may have different CRDT type [ContainerType].
//! Each [Op] has an associated container. It's the [Container]'s responsibility to
//! calculate the state from the [Op]s.
//!
//! Every [Container] can take a [Snapshot], which contains [crate::LoroValue] that describes the state.
//!
use crate::{
    event::{Observer, ObserverHandler, SubscriptionID},
    hierarchy::Hierarchy,
    log_store::ImportContext,
    op::{InnerContent, RemoteContent, RichOp},
    version::PatchedVersionVector,
    InternalString, LoroError, LoroValue, ID,
};

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use std::{
    any::Any,
    fmt::{Debug, Display},
};

use self::pool_mapping::StateContent;

pub mod pool_mapping;
pub mod registry;

pub mod list;
pub mod map;
mod pool;
pub mod text;

pub use registry::ContainerIdx;
// Note: It will be encoded into binary format, so the order of its fields should not be changed.
#[cfg_attr(feature = "test_utils", derive(arbitrary::Arbitrary))]
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy, Serialize, Deserialize)]
pub enum ContainerType {
    /// See [`crate::text::TextContent`]
    Text,
    Map,
    List,
    // TODO: Users can define their own container types.
    // Custom(u16),
}

impl Display for ContainerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ContainerType::Text => "Text",
            ContainerType::Map => "Map",
            ContainerType::List => "List",
        })
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

pub trait ContainerTrait: Debug + Any + Unpin + Send + Sync {
    fn id(&self) -> &ContainerID;
    fn idx(&self) -> ContainerIdx;
    fn type_(&self) -> ContainerType;
    fn get_value(&self) -> LoroValue;

    /// Initialize the pool mapping in current state for this container
    fn initialize_pool_mapping(&mut self);

    /// Encode and release the pool mapping, and return the encoded bytes.
    fn encode_and_release_pool_mapping(&mut self) -> StateContent;

    /// Convert an op content to new op content(s) that includes the data of the new state of the pool mapping.
    fn to_export_snapshot(
        &mut self,
        content: &InnerContent,
        gc: bool,
    ) -> SmallVec<[InnerContent; 1]>;

    /// Decode the pool mapping from the bytes and apply it to the container.
    fn to_import_snapshot(
        &mut self,
        state_content: StateContent,
        hierarchy: &mut Hierarchy,
        ctx: &mut ImportContext,
    );

    /// convert an op content to exported format that includes the raw data
    fn to_export(&mut self, content: InnerContent, gc: bool) -> SmallVec<[RemoteContent; 1]>;

    /// convert an op content to compact imported format
    fn to_import(&mut self, content: RemoteContent) -> InnerContent;

    /// Initialize tracker at the target version
    fn tracker_init(&mut self, vv: &PatchedVersionVector);

    /// Tracker need to checkout to target version in order to apply the op.
    fn tracker_checkout(&mut self, vv: &PatchedVersionVector);

    /// Apply the op to the tracker.
    ///
    /// Here we have not updated the container state yet. Because we
    /// need to calculate the effect of the op for [crate::List] and
    /// [crate::Text] by using tracker.  
    fn track_apply(
        &mut self,
        hierarchy: &mut Hierarchy,
        op: &RichOp,
        import_context: &mut ImportContext,
    );

    /// Apply the effect of the op directly to the state.
    fn update_state_directly(
        &mut self,
        hierarchy: &mut Hierarchy,
        op: &RichOp,
        import_context: &mut ImportContext,
    );
    /// Make tracker iterate over the target spans and apply the calculated
    /// effects to the container state
    fn apply_tracked_effects_from(
        &mut self,
        hierarchy: &mut Hierarchy,
        import_context: &mut ImportContext,
    );

    fn subscribe(
        &self,
        hierarchy: &mut Hierarchy,
        handler: ObserverHandler,
        deep: bool,
        once: bool,
    ) -> SubscriptionID {
        let observer = Observer::new_container(handler, self.id().clone())
            .with_deep(deep)
            .with_once(once);
        hierarchy.subscribe(observer)
    }

    fn unsubscribe(&self, hierarchy: &mut Hierarchy, subscription: SubscriptionID) {
        hierarchy.unsubscribe(subscription);
    }
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
        id: ID,
        container_type: ContainerType,
    },
}

impl Display for ContainerID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerID::Root {
                name,
                container_type,
            } => f.write_fmt(format_args!("/{}:{}", name, container_type))?,
            ContainerID::Normal { id, container_type } => {
                f.write_fmt(format_args!("{}:{}", id, container_type))?
            }
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
                id: ID {
                    counter,
                    client_id: client,
                },
                container_type,
            })
        }
    }
}

pub enum ContainerIdRaw {
    Root { name: InternalString },
    Normal { id: ID },
}

impl<T: Into<InternalString>> From<T> for ContainerIdRaw {
    fn from(value: T) -> Self {
        ContainerIdRaw::Root { name: value.into() }
    }
}

impl From<ID> for ContainerIdRaw {
    fn from(id: ID) -> Self {
        ContainerIdRaw::Normal { id }
    }
}

impl From<&ContainerID> for ContainerIdRaw {
    fn from(id: &ContainerID) -> Self {
        match id {
            ContainerID::Root { name, .. } => ContainerIdRaw::Root { name: name.clone() },
            ContainerID::Normal { id, .. } => ContainerIdRaw::Normal { id: *id },
        }
    }
}

impl From<ContainerID> for ContainerIdRaw {
    fn from(id: ContainerID) -> Self {
        match id {
            ContainerID::Root { name, .. } => ContainerIdRaw::Root { name },
            ContainerID::Normal { id, .. } => ContainerIdRaw::Normal { id },
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
            ContainerIdRaw::Normal { id } => ContainerID::Normal { id, container_type },
        }
    }
}

impl ContainerID {
    #[inline]
    pub fn new_normal(id: ID, container_type: ContainerType) -> Self {
        ContainerID::Normal { id, container_type }
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
