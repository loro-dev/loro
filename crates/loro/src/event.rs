//! Loro event handling.
use enum_as_inner::EnumAsInner;
use loro_internal::container::ContainerID;
use loro_internal::delta::TreeDiff;
use loro_internal::event::EventTriggerKind;
use loro_internal::handler::{TextDelta, ValueOrHandler};
use loro_internal::FxHashMap;
use loro_internal::{
    event::{Diff as DiffInner, Index},
    ContainerDiff as ContainerDiffInner, DiffEvent as DiffEventInner,
};
use std::sync::Arc;

use crate::ValueOrContainer;

/// A subscriber to the event.
pub type Subscriber = Arc<dyn (for<'a> Fn(DiffEvent<'a>)) + Send + Sync>;

/// An event that is triggered by a change in the state of a [super::LoroDoc].
#[derive(Debug)]
pub struct DiffEvent<'a> {
    /// How the event is triggered.
    pub triggered_by: EventTriggerKind,
    /// The origin of the event.
    pub origin: &'a str,
    /// The current receiver of the event.
    pub current_target: Option<ContainerID>,
    /// The diffs of the event.
    pub events: Vec<ContainerDiff<'a>>,
}

/// A diff of a container.
#[derive(Debug)]
pub struct ContainerDiff<'a> {
    /// The target container id of the diff.
    pub target: &'a ContainerID,
    /// The path of the diff.
    pub path: &'a [(ContainerID, Index)],
    /// The diff
    pub diff: Diff<'a>,
}

/// A concrete diff.
#[derive(Debug, EnumAsInner)]
pub enum Diff<'a> {
    /// A list diff.
    List(Vec<ListDiffItem>),
    /// A text diff.
    Text(Vec<TextDelta>),
    /// A map diff.
    Map(MapDelta<'a>),
    /// A tree diff.
    Tree(&'a TreeDiff),
    #[cfg(feature = "counter")]
    /// A counter diff.
    Counter(i64),
}

/// A list diff item.
///
/// We use a `Vec<ListDiffItem>` to represent a list diff.
///
/// Each item can be either an insert, delete, or retain.
///
/// ## Example
///
/// `[Retain(3), Delete(1), Insert{insert: [Value(1), Value(2)], is_move: false}]`
///
/// It means that the list has 3 elements that are not changed, 1 element is deleted, and 2 elements are inserted.
///
/// If the original list is [1, 2, 3, 4, 5], the list after the diff is [1, 2, 3, 1, 2, 5].
#[derive(Debug)]
pub enum ListDiffItem {
    /// Insert a new element into the list.
    Insert {
        /// The new elements to insert.
        insert: Vec<ValueOrContainer>,
        /// Whether the new elements are created by moving
        is_move: bool,
    },
    /// Delete n elements from the list at the current index.
    Delete {
        /// The number of elements to delete.
        delete: usize,
    },
    /// Retain n elements in the list.
    ///
    /// This is used to keep the current index unchanged.
    Retain {
        /// The number of elements to retain.
        retain: usize,
    },
}

/// A map delta.
#[derive(Debug)]
pub struct MapDelta<'a> {
    /// All the updated keys and their new values.
    pub updated: FxHashMap<&'a str, Option<ValueOrContainer>>,
}

impl<'a> From<DiffEventInner<'a>> for DiffEvent<'a> {
    fn from(value: DiffEventInner<'a>) -> Self {
        DiffEvent {
            triggered_by: value.event_meta.by,
            origin: &value.event_meta.origin,
            current_target: value.current_target,
            events: value.events.iter().map(|&diff| diff.into()).collect(),
        }
    }
}

impl<'a> From<&'a ContainerDiffInner> for ContainerDiff<'a> {
    fn from(value: &'a ContainerDiffInner) -> Self {
        ContainerDiff {
            target: &value.id,
            path: &value.path,
            diff: (&value.diff).into(),
        }
    }
}

impl<'a> From<&'a DiffInner> for Diff<'a> {
    fn from(value: &'a DiffInner) -> Self {
        match value {
            DiffInner::List(l) => {
                let mut ans = Vec::new();
                for item in l.iter() {
                    match item {
                        delta::DeltaItem::Retain { len, .. } => {
                            ans.push(ListDiffItem::Retain { retain: *len });
                        }
                        delta::DeltaItem::Replace {
                            value,
                            delete,
                            attr,
                        } => {
                            if value.len() > 0 {
                                ans.push(ListDiffItem::Insert {
                                    insert: value
                                        .iter()
                                        .map(|v| ValueOrContainer::from(v.clone()))
                                        .collect(),
                                    is_move: attr.from_move,
                                });
                            }
                            if *delete > 0 {
                                ans.push(ListDiffItem::Delete { delete: *delete });
                            }
                        }
                    }
                }

                Diff::List(ans)
            }
            DiffInner::Map(m) => Diff::Map(MapDelta {
                updated: m
                    .updated
                    .iter()
                    .map(|(k, v)| (k.as_ref(), v.value.clone().map(|v| v.into())))
                    .collect(),
            }),
            DiffInner::Text(t) => {
                let text = TextDelta::from_text_diff(t.iter());
                Diff::Text(text)
            }
            DiffInner::Tree(t) => Diff::Tree(t),
            _ => todo!(),
        }
    }
}

impl From<ValueOrHandler> for ValueOrContainer {
    fn from(value: ValueOrHandler) -> Self {
        match value {
            ValueOrHandler::Value(v) => ValueOrContainer::Value(v),
            ValueOrHandler::Handler(h) => ValueOrContainer::Container(h.into()),
        }
    }
}
