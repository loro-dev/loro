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
    op::{InnerContent, RawOpContent, RichOp},
    InternalString, LoroValue, VersionVector, ID,
};

use smallvec::SmallVec;

use std::{any::Any, fmt::Debug};

use self::pool_mapping::StateContent;

pub mod pool_mapping;
pub mod registry;

pub mod list;
pub mod map;
mod pool;
pub mod text;

use registry::ContainerIdx;

pub use loro_common::ContainerType;

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
    fn to_export(&mut self, content: InnerContent, gc: bool) -> SmallVec<[RawOpContent; 1]>;

    /// convert an op content to compact imported format
    fn to_import(&mut self, content: RawOpContent) -> InnerContent;

    /// Initialize tracker at the target version
    fn tracker_init(&mut self, vv: &VersionVector);

    /// Tracker need to checkout to target version in order to apply the op.
    fn tracker_checkout(&mut self, vv: &VersionVector);

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

pub use loro_common::ContainerID;

pub enum ContainerIdRaw {
    Root { name: InternalString },
    Normal { id: ID },
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
