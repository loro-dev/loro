mod value;

pub use value::{ContainerID, ContainerType, LoroValue, LoroValueLike};
mod doc;
pub use doc::LoroDoc;
mod container;
pub use container::{
    ContainerIdLike, Cursor, LoroCounter, LoroList, LoroMap, LoroMovableList, LoroText, LoroTree,
    LoroUnknown,
};
mod event;
pub use event::{
    ContainerDiff, Diff, DiffEvent, Index, ListDiffItem, MapDelta, PathItem, Subscriber, TextDelta,
    TreeDiff, TreeDiffItem, TreeExternalDiff,
};
mod version;
pub use loro::{
    cursor::Side, Counter, EventTriggerKind, Frontiers, IdSpan, Lamport, LoroError, PeerID, SubID,
    TreeID, VersionVector, ID,
};

// https://github.com/mozilla/uniffi-rs/issues/1372
pub trait ValueOrContainer: Send + Sync {
    fn is_value(&self) -> bool;
    fn is_container(&self) -> bool;
    fn as_value(&self) -> Option<LoroValue>;
    fn as_container(&self) -> Option<ContainerID>;
}

impl ValueOrContainer for loro::ValueOrContainer {
    fn is_value(&self) -> bool {
        loro::ValueOrContainer::is_value(self)
    }

    fn is_container(&self) -> bool {
        loro::ValueOrContainer::is_container(self)
    }

    fn as_value(&self) -> Option<LoroValue> {
        loro::ValueOrContainer::as_value(self)
            .cloned()
            .map(LoroValue::from)
    }

    fn as_container(&self) -> Option<ContainerID> {
        loro::ValueOrContainer::as_container(self).map(|c| c.id().into())
    }
}
