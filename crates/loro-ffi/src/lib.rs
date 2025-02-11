mod value;

pub use loro::{
    cursor::Side, undo::UndoOrRedo, CannotFindRelativePosition, ChangeTravelError, Counter,
    CounterSpan, EventTriggerKind, ExpandType, FractionalIndex, IdLp, IdSpan, JsonChange,
    JsonFutureOp, JsonFutureOpWrapper, JsonListOp, JsonMapOp, JsonMovableListOp, JsonOp,
    JsonOpContent, JsonPathError, JsonSchema, JsonTextOp, JsonTreeOp, Lamport, LoroEncodeError,
    LoroError, PeerID, StyleConfig, TreeID, UpdateOptions, UpdateTimeoutError, ID,
};
pub use std::cmp::Ordering;
pub use value::{ContainerID, ContainerType, LoroValue, LoroValueLike};
mod doc;
pub use doc::{
    decode_import_blob_meta, ChangeAncestorsTraveler, ChangeMeta, CommitOptions, ContainerPath,
    ExportMode, FrontiersOrID, ImportBlobMetadata, ImportStatus, JsonSchemaLike,
    LocalUpdateCallback, LoroDoc, PosQueryResult, Subscription, Unsubscriber,
};
mod container;
pub use container::{
    Container, ContainerIdLike, Cursor, LoroCounter, LoroList, LoroMap, LoroMovableList, LoroText,
    LoroTree, LoroUnknown, TreeParentId,
};
mod event;
pub use event::{
    ContainerDiff, ContainerIDAndDiff, Diff, DiffBatch, DiffEvent, Index, ListDiffItem, MapDelta,
    PathItem, Subscriber, TextDelta, TreeDiff, TreeDiffItem, TreeExternalDiff,
};
mod undo;
pub use undo::{AbsolutePosition, CursorWithPos, OnPop, OnPush, UndoItemMeta, UndoManager};
mod config;
pub use config::{Configure, StyleConfigMap};
mod version;
pub use version::{Frontiers, VersionVector, VersionVectorDiff};
mod awareness;
pub use awareness::{Awareness, AwarenessPeerUpdate, PeerInfo};

#[derive(Debug, Clone)]
pub enum ValueOrContainer {
    Value { value: LoroValue },
    Container { container: Container },
}

impl From<ValueOrContainer> for loro::ValueOrContainer {
    fn from(value: ValueOrContainer) -> Self {
        match value {
            ValueOrContainer::Value { value } => loro::ValueOrContainer::Value(value.into()),
            ValueOrContainer::Container { container } => {
                loro::ValueOrContainer::Container(container.into())
            }
        }
    }
}

impl From<loro::ValueOrContainer> for ValueOrContainer {
    fn from(value: loro::ValueOrContainer) -> Self {
        match value {
            loro::ValueOrContainer::Value(v) => ValueOrContainer::Value { value: v.into() },
            loro::ValueOrContainer::Container(c) => ValueOrContainer::Container {
                container: c.into(),
            },
        }
    }
}
