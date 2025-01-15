//! Loro event handling.
use delta::array_vec::ArrayVec;
use delta::DeltaRope;
use enum_as_inner::EnumAsInner;
use loro_common::IdLp;
use loro_internal::container::ContainerID;
pub use loro_internal::delta::TreeDiff;
use loro_internal::delta::{ResolvedMapDelta, ResolvedMapValue};
use loro_internal::event::{EventTriggerKind, ListDeltaMeta};
use loro_internal::handler::{TextDelta, ValueOrHandler};
use loro_internal::undo::DiffBatch as InnerDiffBatch;
use loro_internal::{
    event::{Diff as DiffInner, Index},
    ContainerDiff as ContainerDiffInner, DiffEvent as DiffEventInner,
};
use loro_internal::{FxHashMap, ListDiffInsertItem};
use std::borrow::Cow;
use std::ops::Deref;
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
    /// Whether the diff is from unknown container.
    pub is_unknown: bool,
    /// The diff
    pub diff: Diff<'a>,
}

/// A concrete diff.
#[derive(Debug, EnumAsInner, Clone)]
pub enum Diff<'a> {
    /// A list diff.
    List(Vec<ListDiffItem>),
    /// A text diff.
    Text(Vec<TextDelta>),
    /// A map diff.
    Map(MapDelta<'a>),
    /// A tree diff.
    Tree(Cow<'a, TreeDiff>),
    #[cfg(feature = "counter")]
    /// A counter diff.
    Counter(f64),
    /// An unknown diff.
    Unknown,
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct MapDelta<'a> {
    /// All the updated keys and their new values.
    pub updated: FxHashMap<Cow<'a, str>, Option<ValueOrContainer>>,
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
            is_unknown: value.is_unknown,
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
                    .map(|(k, v)| (Cow::Borrowed(k.as_str()), v.value.clone().map(|v| v.into())))
                    .collect(),
            }),
            DiffInner::Text(t) => {
                let text = TextDelta::from_text_diff(t.iter());
                Diff::Text(text)
            }
            DiffInner::Tree(t) => Diff::Tree(Cow::Borrowed(t)),
            #[cfg(feature = "counter")]
            DiffInner::Counter(c) => Diff::Counter(*c),
            DiffInner::Unknown => Diff::Unknown,
            _ => todo!(),
        }
    }
}

impl From<DiffInner> for Diff<'static> {
    fn from(value: DiffInner) -> Self {
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
                    .map(|(k, v)| (Cow::Owned(k.to_string()), v.value.clone().map(|v| v.into())))
                    .collect(),
            }),
            DiffInner::Text(t) => {
                let text = TextDelta::from_text_diff(t.iter());
                Diff::Text(text)
            }
            DiffInner::Tree(t) => Diff::Tree(Cow::Owned(t.clone())),
            #[cfg(feature = "counter")]
            DiffInner::Counter(c) => Diff::Counter(c),
            DiffInner::Unknown => Diff::Unknown,
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

/// A batch of diffs.
#[derive(Default, Clone)]
pub struct DiffBatch {
    cid_to_events: FxHashMap<ContainerID, Diff<'static>>,
    order: Vec<ContainerID>,
}

impl std::fmt::Debug for DiffBatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let v: Vec<(&ContainerID, &Diff<'static>)> = self.iter().collect();
        write!(f, "{:#?}", v)
    }
}

impl DiffBatch {
    /// Returns an iterator over the diffs in this batch, in the order they were added.
    ///
    /// The iterator yields tuples of `(&ContainerID, &Diff)` where:
    /// - `ContainerID` is the ID of the container that was modified
    /// - `Diff` contains the actual changes made to that container
    ///
    /// The order of the diffs is preserved from when they were originally added to the batch.
    pub fn iter(&self) -> impl Iterator<Item = (&ContainerID, &Diff<'static>)> {
        self.order.iter().map(|id| (id, &self.cid_to_events[id]))
    }

    /// Push a new event to the batch.
    ///
    /// If the cid already exists in the batch, return Err
    pub fn push(&mut self, cid: ContainerID, diff: Diff<'static>) -> Result<(), Diff<'static>> {
        match self.cid_to_events.entry(cid.clone()) {
            std::collections::hash_map::Entry::Occupied(_) => Err(diff),
            std::collections::hash_map::Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(diff);
                self.order.push(cid);
                Ok(())
            }
        }
    }
}

impl From<InnerDiffBatch> for DiffBatch {
    fn from(value: InnerDiffBatch) -> Self {
        let mut map =
            FxHashMap::with_capacity_and_hasher(value.cid_to_events.len(), Default::default());
        for (id, diff) in value.cid_to_events.into_iter() {
            map.insert(id.clone(), diff.into());
        }

        DiffBatch {
            cid_to_events: map,
            order: value.order,
        }
    }
}

impl From<Diff<'static>> for DiffInner {
    fn from(value: Diff<'static>) -> Self {
        match value {
            Diff::List(vec) => {
                let mut ans: DeltaRope<ListDiffInsertItem, ListDeltaMeta> = DeltaRope::new();
                for item in vec.iter() {
                    match item {
                        ListDiffItem::Insert { insert, is_move } => {
                            for item in ArrayVec::from_many(
                                insert.iter().map(|v| v.clone().into_value_or_handler()),
                            ) {
                                ans.push_insert(
                                    item,
                                    ListDeltaMeta {
                                        from_move: *is_move,
                                    },
                                );
                            }
                        }
                        ListDiffItem::Delete { delete } => {
                            ans.push_delete(*delete);
                        }
                        ListDiffItem::Retain { retain } => {
                            ans.push_retain(*retain, ListDeltaMeta { from_move: false });
                        }
                    }
                }

                DiffInner::List(ans)
            }
            Diff::Text(t) => {
                let text = TextDelta::into_text_diff(t.into_iter());
                DiffInner::Text(text)
            }
            Diff::Map(map_delta) => DiffInner::Map(ResolvedMapDelta {
                updated: map_delta
                    .updated
                    .into_iter()
                    .map(|(k, v)| {
                        (
                            k.deref().into(),
                            ResolvedMapValue {
                                value: v.map(|v| v.into_value_or_handler()),
                                idlp: IdLp::new(0, 0),
                            },
                        )
                    })
                    .collect(),
            }),
            Diff::Tree(cow) => DiffInner::Tree(cow.into_owned()),
            #[cfg(feature = "counter")]
            Diff::Counter(c) => DiffInner::Counter(c),
            Diff::Unknown => DiffInner::Unknown,
        }
    }
}

impl From<DiffBatch> for InnerDiffBatch {
    fn from(value: DiffBatch) -> Self {
        let mut map =
            FxHashMap::with_capacity_and_hasher(value.cid_to_events.len(), Default::default());
        for (id, diff) in value.cid_to_events.into_iter() {
            map.insert(id.clone(), diff.into());
        }

        InnerDiffBatch {
            cid_to_events: map,
            order: value.order,
        }
    }
}
