mod value;

use loro::Container;
pub use loro::{
    cursor::Side, undo::UndoOrRedo, CannotFindRelativePosition, Counter, CounterSpan,
    EventTriggerKind, ExpandType, FractionalIndex, IdLp, IdSpan, JsonChange, JsonFutureOp,
    JsonFutureOpWrapper, JsonListOp, JsonMapOp, JsonMovableListOp, JsonOp, JsonOpContent,
    JsonPathError, JsonSchema, JsonTextOp, JsonTreeOp, Lamport, LoroError, PeerID, StyleConfig,
    TreeID, ID,
};
pub use std::cmp::Ordering;
use std::sync::Arc;
pub use value::{ContainerID, ContainerType, LoroValue, LoroValueLike};
mod doc;
pub use doc::{
    ChangeMeta, CommitOptions, ContainerPath, ExportMode, ImportBlobMetadata, JsonSchemaLike,
    LocalUpdateCallback, LoroDoc, PosQueryResult, Subscription, Unsubscriber,
};
mod container;
pub use container::{
    ContainerIdLike, Cursor, LoroCounter, LoroList, LoroMap, LoroMovableList, LoroText, LoroTree,
    LoroUnknown, TreeParentId,
};
mod event;
pub use event::{
    ContainerDiff, Diff, DiffEvent, Index, ListDiffItem, MapDelta, PathItem, Subscriber, TextDelta,
    TreeDiff, TreeDiffItem, TreeExternalDiff,
};
mod undo;
pub use undo::{AbsolutePosition, CursorWithPos, OnPop, OnPush, UndoItemMeta, UndoManager};
mod config;
pub use config::{Configure, StyleConfigMap};
mod version;
pub use version::{Frontiers, VersionVector, VersionVectorDiff};
mod awareness;
pub use awareness::{Awareness, AwarenessPeerUpdate, PeerInfo};

// https://github.com/mozilla/uniffi-rs/issues/1372
pub trait ValueOrContainer: Send + Sync {
    fn is_value(&self) -> bool;
    fn is_container(&self) -> bool;
    fn as_value(&self) -> Option<LoroValue>;
    fn as_container(&self) -> Option<ContainerID>;
    fn as_loro_list(&self) -> Option<Arc<LoroList>>;
    fn as_loro_text(&self) -> Option<Arc<LoroText>>;
    fn as_loro_map(&self) -> Option<Arc<LoroMap>>;
    fn as_loro_movable_list(&self) -> Option<Arc<LoroMovableList>>;
    fn as_loro_tree(&self) -> Option<Arc<LoroTree>>;
    fn as_loro_counter(&self) -> Option<Arc<LoroCounter>>;
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

    fn as_loro_list(&self) -> Option<Arc<LoroList>> {
        match self {
            loro::ValueOrContainer::Container(Container::List(list)) => {
                Some(Arc::new(LoroList { list: list.clone() }))
            }
            _ => None,
        }
    }

    fn as_loro_text(&self) -> Option<Arc<LoroText>> {
        match self {
            loro::ValueOrContainer::Container(Container::Text(c)) => {
                Some(Arc::new(LoroText { text: c.clone() }))
            }
            _ => None,
        }
    }

    fn as_loro_map(&self) -> Option<Arc<LoroMap>> {
        match self {
            loro::ValueOrContainer::Container(Container::Map(c)) => {
                Some(Arc::new(LoroMap { map: c.clone() }))
            }
            _ => None,
        }
    }

    fn as_loro_movable_list(&self) -> Option<Arc<LoroMovableList>> {
        match self {
            loro::ValueOrContainer::Container(Container::MovableList(c)) => {
                Some(Arc::new(LoroMovableList { list: c.clone() }))
            }
            _ => None,
        }
    }

    fn as_loro_tree(&self) -> Option<Arc<LoroTree>> {
        match self {
            loro::ValueOrContainer::Container(Container::Tree(c)) => {
                Some(Arc::new(LoroTree { tree: c.clone() }))
            }
            _ => None,
        }
    }

    fn as_loro_counter(&self) -> Option<Arc<LoroCounter>> {
        match self {
            loro::ValueOrContainer::Container(Container::Counter(c)) => {
                Some(Arc::new(LoroCounter { counter: c.clone() }))
            }
            _ => None,
        }
    }
}
