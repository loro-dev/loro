mod value;

use loro::Container;
pub use loro::{
    cursor::Side, undo::UndoOrRedo, CannotFindRelativePosition, ChangeTravelError, Counter,
    CounterSpan, EventTriggerKind, ExpandType, FractionalIndex, IdLp, IdSpan, JsonChange,
    JsonFutureOp, JsonFutureOpWrapper, JsonListOp, JsonMapOp, JsonMovableListOp, JsonOp,
    JsonOpContent, JsonPathError, JsonSchema, JsonTextOp, JsonTreeOp, Lamport, LoroEncodeError,
    LoroError, PeerID, StyleConfig, TreeID, UpdateOptions, UpdateTimeoutError, ID, LORO_VERSION,
};
pub use std::cmp::Ordering;
use std::sync::Arc;
pub use value::{ContainerID, ContainerType, LoroValue, LoroValueLike};
mod doc;
pub use doc::*;
mod container;
pub use container::*;
mod event;
pub use event::*;
mod undo;
pub use undo::*;
mod config;
pub use config::{Configure, StyleConfigMap};
mod version;
pub use version::{Frontiers, VersionVector, VersionVectorDiff};
mod awareness;
pub use awareness::*;
mod ephemeral;
pub use ephemeral::*;

// https://github.com/mozilla/uniffi-rs/issues/1372
pub trait ValueOrContainer: Send + Sync {
    fn is_value(&self) -> bool;
    fn is_container(&self) -> bool;
    fn as_value(&self) -> Option<LoroValue>;
    fn container_type(&self) -> Option<ContainerType>;
    fn as_container(&self) -> Option<ContainerID>;
    fn as_loro_list(&self) -> Option<Arc<LoroList>>;
    fn as_loro_text(&self) -> Option<Arc<LoroText>>;
    fn as_loro_map(&self) -> Option<Arc<LoroMap>>;
    fn as_loro_movable_list(&self) -> Option<Arc<LoroMovableList>>;
    fn as_loro_tree(&self) -> Option<Arc<LoroTree>>;
    fn as_loro_counter(&self) -> Option<Arc<LoroCounter>>;
    fn as_loro_unknown(&self) -> Option<Arc<LoroUnknown>>;
}

impl ValueOrContainer for loro::ValueOrContainer {
    fn is_value(&self) -> bool {
        loro::ValueOrContainer::is_value(self)
    }

    fn is_container(&self) -> bool {
        loro::ValueOrContainer::is_container(self)
    }

    fn container_type(&self) -> Option<ContainerType> {
        loro::ValueOrContainer::as_container(self).map(|c| c.id().container_type().into())
    }

    fn as_value(&self) -> Option<LoroValue> {
        loro::ValueOrContainer::as_value(self)
            .cloned()
            .map(LoroValue::from)
    }

    // TODO: pass Container to Swift
    fn as_container(&self) -> Option<ContainerID> {
        loro::ValueOrContainer::as_container(self).map(|c| c.id().into())
    }

    fn as_loro_list(&self) -> Option<Arc<LoroList>> {
        match self {
            loro::ValueOrContainer::Container(Container::List(list)) => Some(Arc::new(LoroList {
                inner: list.clone(),
            })),
            _ => None,
        }
    }

    fn as_loro_text(&self) -> Option<Arc<LoroText>> {
        match self {
            loro::ValueOrContainer::Container(Container::Text(c)) => {
                Some(Arc::new(LoroText { inner: c.clone() }))
            }
            _ => None,
        }
    }

    fn as_loro_map(&self) -> Option<Arc<LoroMap>> {
        match self {
            loro::ValueOrContainer::Container(Container::Map(c)) => {
                Some(Arc::new(LoroMap { inner: c.clone() }))
            }
            _ => None,
        }
    }

    fn as_loro_movable_list(&self) -> Option<Arc<LoroMovableList>> {
        match self {
            loro::ValueOrContainer::Container(Container::MovableList(c)) => {
                Some(Arc::new(LoroMovableList { inner: c.clone() }))
            }
            _ => None,
        }
    }

    fn as_loro_tree(&self) -> Option<Arc<LoroTree>> {
        match self {
            loro::ValueOrContainer::Container(Container::Tree(c)) => {
                Some(Arc::new(LoroTree { inner: c.clone() }))
            }
            _ => None,
        }
    }

    fn as_loro_counter(&self) -> Option<Arc<LoroCounter>> {
        match self {
            loro::ValueOrContainer::Container(Container::Counter(c)) => {
                Some(Arc::new(LoroCounter { inner: c.clone() }))
            }
            _ => None,
        }
    }

    fn as_loro_unknown(&self) -> Option<Arc<LoroUnknown>> {
        match self {
            loro::ValueOrContainer::Container(Container::Unknown(c)) => {
                Some(Arc::new(LoroUnknown { inner: c.clone() }))
            }
            _ => None,
        }
    }
}

fn convert_trait_to_v_or_container<T: AsRef<dyn ValueOrContainer>>(i: T) -> loro::ValueOrContainer {
    let v = i.as_ref();
    if v.is_value() {
        loro::ValueOrContainer::Value(v.as_value().unwrap().into())
    } else {
        let container = match v.container_type().unwrap() {
            ContainerType::List => Container::List((*v.as_loro_list().unwrap()).clone().inner),
            ContainerType::Text => Container::Text((*v.as_loro_text().unwrap()).clone().inner),
            ContainerType::Map => Container::Map((*v.as_loro_map().unwrap()).clone().inner),
            ContainerType::MovableList => {
                Container::MovableList((*v.as_loro_movable_list().unwrap()).clone().inner)
            }
            ContainerType::Tree => Container::Tree((*v.as_loro_tree().unwrap()).clone().inner),
            ContainerType::Counter => {
                Container::Counter((*v.as_loro_counter().unwrap()).clone().inner)
            }
            ContainerType::Unknown { kind: _ } => {
                Container::Unknown((*v.as_loro_unknown().unwrap()).clone().inner)
            }
        };
        loro::ValueOrContainer::Container(container)
    }
}

pub fn get_version() -> String {
    LORO_VERSION.to_string()
}
