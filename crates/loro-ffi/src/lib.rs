mod value;

use loro::Container;
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
mod undo;
pub use undo::{AbsolutePosition, CursorWithPos, OnPop, OnPush, UndoItemMeta, UndoManager};
mod version;
pub use loro::{
    cursor::Side, undo::UndoOrRedo, Counter, CounterSpan, EventTriggerKind, Frontiers, IdSpan,
    Lamport, LoroError, PeerID, SubID, TreeID, VersionVector, ID,
};

// https://github.com/mozilla/uniffi-rs/issues/1372
pub trait ValueOrContainer: Send + Sync {
    fn is_value(&self) -> bool;
    fn is_container(&self) -> bool;
    fn as_value(&self) -> Option<LoroValue>;
    fn as_container(&self) -> Option<ContainerID>;
    fn as_loro_list(&self) -> Option<LoroList>;
    fn as_loro_text(&self) -> Option<LoroText>;
    fn as_loro_map(&self) -> Option<LoroMap>;
    fn as_loro_movable_list(&self) -> Option<LoroMovableList>;
    fn as_loro_tree(&self) -> Option<LoroTree>;
    fn as_loro_counter(&self) -> Option<LoroCounter>;
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

    fn as_loro_list(&self) -> Option<LoroList> {
        match self {
            loro::ValueOrContainer::Container(Container::List(list)) => {
                Some(LoroList { list: list.clone() })
            }
            _ => None,
        }
    }

    fn as_loro_text(&self) -> Option<LoroText> {
        match self {
            loro::ValueOrContainer::Container(Container::Text(c)) => {
                Some(LoroText { text: c.clone() })
            }
            _ => None,
        }
    }

    fn as_loro_map(&self) -> Option<LoroMap> {
        match self {
            loro::ValueOrContainer::Container(Container::Map(c)) => {
                Some(LoroMap { map: c.clone() })
            }
            _ => None,
        }
    }

    fn as_loro_movable_list(&self) -> Option<LoroMovableList> {
        match self {
            loro::ValueOrContainer::Container(Container::MovableList(c)) => {
                Some(LoroMovableList { list: c.clone() })
            }
            _ => None,
        }
    }

    fn as_loro_tree(&self) -> Option<LoroTree> {
        match self {
            loro::ValueOrContainer::Container(Container::Tree(c)) => {
                Some(LoroTree { tree: c.clone() })
            }
            _ => None,
        }
    }

    fn as_loro_counter(&self) -> Option<LoroCounter> {
        match self {
            loro::ValueOrContainer::Container(Container::Counter(c)) => {
                Some(LoroCounter { counter: c.clone() })
            }
            _ => None,
        }
    }
}
