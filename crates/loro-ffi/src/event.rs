use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex},
};

use loro::{EventTriggerKind, FractionalIndex, TreeID};

use crate::{
    convert_trait_to_v_or_container, ContainerID, LoroValue, TreeParentId, ValueOrContainer,
};

pub trait Subscriber: Sync + Send {
    fn on_diff(&self, diff: DiffEvent);
}

pub struct DiffEvent {
    /// How the event is triggered.
    pub triggered_by: EventTriggerKind,
    /// The origin of the event.
    pub origin: String,
    /// The current receiver of the event.
    pub current_target: Option<ContainerID>,
    /// The diffs of the event.
    pub events: Vec<ContainerDiff>,
}

impl From<loro::event::DiffEvent<'_>> for DiffEvent {
    fn from(diff_event: loro::event::DiffEvent) -> Self {
        Self {
            triggered_by: diff_event.triggered_by,
            origin: diff_event.origin.to_string(),
            current_target: diff_event.current_target.map(|v| v.into()),
            events: diff_event.events.iter().map(ContainerDiff::from).collect(),
        }
    }
}

pub struct PathItem {
    pub container: ContainerID,
    pub index: Index,
}

/// A diff of a container.
pub struct ContainerDiff {
    /// The target container id of the diff.
    pub target: ContainerID,
    /// The path of the diff.
    pub path: Vec<PathItem>,
    /// Whether the diff is from unknown container.
    pub is_unknown: bool,
    /// The diff
    pub diff: Diff,
}

#[derive(Debug, Clone)]
pub enum Index {
    Key { key: String },
    Seq { index: u32 },
    Node { target: TreeID },
}

pub enum Diff {
    /// A list diff.
    List { diff: Vec<ListDiffItem> },
    /// A text diff.
    Text { diff: Vec<TextDelta> },
    /// A map diff.
    Map { diff: MapDelta },
    /// A tree diff.
    Tree { diff: TreeDiff },
    /// A counter diff.
    Counter { diff: f64 },
    /// An unknown diff.
    Unknown,
}

pub enum TextDelta {
    Retain {
        retain: u32,
        attributes: Option<HashMap<String, LoroValue>>,
    },
    Insert {
        insert: String,
        attributes: Option<HashMap<String, LoroValue>>,
    },
    Delete {
        delete: u32,
    },
}

impl From<TextDelta> for loro::TextDelta {
    fn from(value: TextDelta) -> Self {
        match value {
            TextDelta::Retain { retain, attributes } => Self::Retain {
                retain: retain as usize,
                attributes: attributes.as_ref().map(|a| {
                    a.iter()
                        .map(|(k, v)| (k.to_string(), v.clone().into()))
                        .collect()
                }),
            },
            TextDelta::Insert { insert, attributes } => Self::Insert {
                insert,
                attributes: attributes.as_ref().map(|a| {
                    a.iter()
                        .map(|(k, v)| (k.to_string(), v.clone().into()))
                        .collect()
                }),
            },
            TextDelta::Delete { delete } => Self::Delete {
                delete: delete as usize,
            },
        }
    }
}

impl From<loro::TextDelta> for TextDelta {
    fn from(value: loro::TextDelta) -> Self {
        match value {
            loro::TextDelta::Retain { retain, attributes } => Self::Retain {
                retain: retain as u32,
                attributes: attributes.as_ref().map(|a| {
                    a.iter()
                        .map(|(k, v)| (k.to_string(), v.clone().into()))
                        .collect()
                }),
            },
            loro::TextDelta::Insert { insert, attributes } => Self::Insert {
                insert,
                attributes: attributes.as_ref().map(|a| {
                    a.iter()
                        .map(|(k, v)| (k.to_string(), v.clone().into()))
                        .collect()
                }),
            },
            loro::TextDelta::Delete { delete } => Self::Delete {
                delete: delete as u32,
            },
        }
    }
}

impl From<ListDiffItem> for loro::event::ListDiffItem {
    fn from(value: ListDiffItem) -> Self {
        match value {
            ListDiffItem::Insert { insert, is_move } => Self::Insert {
                insert: insert
                    .into_iter()
                    .map(convert_trait_to_v_or_container)
                    .collect(),
                is_move,
            },
            ListDiffItem::Delete { delete } => Self::Delete {
                delete: delete as usize,
            },
            ListDiffItem::Retain { retain } => Self::Retain {
                retain: retain as usize,
            },
        }
    }
}

impl From<MapDelta> for loro::event::MapDelta<'static> {
    fn from(value: MapDelta) -> Self {
        loro::event::MapDelta {
            updated: value
                .updated
                .into_iter()
                .map(|(k, v)| (Cow::Owned(k), v.map(convert_trait_to_v_or_container)))
                .collect(),
        }
    }
}

impl From<TreeDiffItem> for loro::TreeDiffItem {
    fn from(value: TreeDiffItem) -> Self {
        let target: TreeID = value.target;
        let action = match value.action {
            TreeExternalDiff::Create {
                parent,
                index,
                fractional_index,
            } => loro::TreeExternalDiff::Create {
                parent: parent.into(),
                index: index as usize,
                position: FractionalIndex::from_hex_string(fractional_index),
            },
            TreeExternalDiff::Move {
                parent,
                index,
                fractional_index,
                old_parent,
                old_index,
            } => loro::TreeExternalDiff::Move {
                parent: parent.into(),
                index: index as usize,
                position: FractionalIndex::from_hex_string(fractional_index),
                old_parent: old_parent.into(),
                old_index: old_index as usize,
            },
            TreeExternalDiff::Delete {
                old_parent,
                old_index,
            } => loro::TreeExternalDiff::Delete {
                old_parent: old_parent.into(),
                old_index: old_index as usize,
            },
        };
        Self { target, action }
    }
}

pub enum ListDiffItem {
    /// Insert a new element into the list.
    Insert {
        /// The new elements to insert.
        insert: Vec<Arc<dyn ValueOrContainer>>,
        /// Whether the new elements are created by moving
        is_move: bool,
    },
    /// Delete n elements from the list at the current index.
    Delete {
        /// The number of elements to delete.
        delete: u32,
    },
    /// Retain n elements in the list.
    ///
    /// This is used to keep the current index unchanged.
    Retain {
        /// The number of elements to retain.
        retain: u32,
    },
}

pub struct MapDelta {
    /// All the updated keys and their new values.
    pub updated: HashMap<String, Option<Arc<dyn ValueOrContainer>>>,
}

pub struct TreeDiff {
    pub diff: Vec<TreeDiffItem>,
}

pub struct TreeDiffItem {
    pub target: TreeID,
    pub action: TreeExternalDiff,
}

pub enum TreeExternalDiff {
    Create {
        parent: TreeParentId,
        index: u32,
        fractional_index: String,
    },
    Move {
        parent: TreeParentId,
        index: u32,
        fractional_index: String,
        old_parent: TreeParentId,
        old_index: u32,
    },
    Delete {
        old_parent: TreeParentId,
        old_index: u32,
    },
}

impl<'a> From<&loro::event::ContainerDiff<'a>> for ContainerDiff {
    fn from(value: &loro::event::ContainerDiff<'a>) -> Self {
        Self {
            target: value.target.into(),
            path: value
                .path
                .iter()
                .map(|(id, index)| PathItem {
                    container: id.into(),
                    index: index.into(),
                })
                .collect(),
            is_unknown: value.is_unknown,
            diff: (&value.diff).into(),
        }
    }
}

impl From<&loro::Index> for Index {
    fn from(value: &loro::Index) -> Self {
        match value {
            loro::Index::Key(key) => Self::Key {
                key: key.to_string(),
            },
            loro::Index::Seq(index) => Self::Seq {
                index: *index as u32,
            },
            loro::Index::Node(target) => Self::Node { target: *target },
        }
    }
}

impl From<Index> for loro::Index {
    fn from(value: Index) -> Self {
        match value {
            Index::Key { key } => Self::Key(key.into()),
            Index::Seq { index } => Self::Seq(index as usize),
            Index::Node { target } => Self::Node(target),
        }
    }
}

impl From<&loro::event::Diff<'_>> for Diff {
    fn from(value: &loro::event::Diff) -> Self {
        match value {
            loro::event::Diff::List(l) => {
                let mut ans = Vec::with_capacity(l.len());
                for item in l.iter() {
                    match item {
                        loro::event::ListDiffItem::Insert { insert, is_move } => {
                            let mut new_insert = Vec::with_capacity(insert.len());
                            for v in insert.iter() {
                                new_insert.push(Arc::new(v.clone()) as Arc<dyn ValueOrContainer>);
                            }
                            ans.push(ListDiffItem::Insert {
                                insert: new_insert,
                                is_move: *is_move,
                            });
                        }
                        loro::event::ListDiffItem::Delete { delete } => {
                            ans.push(ListDiffItem::Delete {
                                delete: *delete as u32,
                            });
                        }
                        loro::event::ListDiffItem::Retain { retain } => {
                            ans.push(ListDiffItem::Retain {
                                retain: *retain as u32,
                            });
                        }
                    }
                }
                Self::List { diff: ans }
            }
            loro::event::Diff::Text(t) => Self::Text {
                diff: t.iter().map(|i| i.clone().into()).collect(),
            },
            loro::event::Diff::Map(m) => {
                let mut updated = HashMap::new();
                for (key, value) in m.updated.iter() {
                    updated.insert(
                        key.to_string(),
                        value
                            .as_ref()
                            .map(|v| Arc::new(v.clone()) as Arc<dyn ValueOrContainer>),
                    );
                }

                Self::Map {
                    diff: MapDelta { updated },
                }
            }
            loro::event::Diff::Tree(t) => {
                let mut diff = Vec::new();
                for item in t.iter() {
                    diff.push(TreeDiffItem {
                        target: item.target,
                        action: match &item.action {
                            loro::TreeExternalDiff::Create {
                                parent,
                                index,
                                position,
                            } => TreeExternalDiff::Create {
                                parent: (*parent).into(),
                                index: *index as u32,
                                fractional_index: position.to_string(),
                            },
                            loro::TreeExternalDiff::Move {
                                parent,
                                index,
                                position,
                                old_parent,
                                old_index,
                            } => TreeExternalDiff::Move {
                                parent: (*parent).into(),
                                index: *index as u32,
                                fractional_index: position.to_string(),
                                old_parent: (*old_parent).into(),
                                old_index: *old_index as u32,
                            },
                            loro::TreeExternalDiff::Delete {
                                old_parent,
                                old_index,
                            } => TreeExternalDiff::Delete {
                                old_parent: (*old_parent).into(),
                                old_index: *old_index as u32,
                            },
                        },
                    });
                }
                Self::Tree {
                    diff: TreeDiff { diff },
                }
            }
            loro::event::Diff::Counter(c) => Self::Counter { diff: *c },
            loro::event::Diff::Unknown => Self::Unknown,
        }
    }
}

impl From<Diff> for loro::event::Diff<'static> {
    fn from(value: Diff) -> Self {
        match value {
            Diff::List { diff } => {
                loro::event::Diff::List(diff.into_iter().map(|i| i.into()).collect())
            }
            Diff::Text { diff } => {
                loro::event::Diff::Text(diff.into_iter().map(|i| i.into()).collect())
            }
            Diff::Map { diff } => loro::event::Diff::Map(diff.into()),
            Diff::Tree { diff } => loro::event::Diff::Tree(Cow::Owned(loro::TreeDiff {
                diff: diff.diff.into_iter().map(|i| i.into()).collect(),
            })),
            Diff::Counter { diff } => loro::event::Diff::Counter(diff),
            Diff::Unknown => loro::event::Diff::Unknown,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DiffBatch(Arc<Mutex<loro::event::DiffBatch>>);

impl DiffBatch {
    pub fn new() -> Self {
        Self(Default::default())
    }

    /// Push a new event to the batch.
    ///
    /// If the cid already exists in the batch, return Err
    pub fn push(&self, cid: ContainerID, diff: Diff) -> Option<Diff> {
        let mut batch = self.0.lock().unwrap();
        if let Err(diff) = batch.push(cid.into(), diff.into()) {
            Some((&diff).into())
        } else {
            None
        }
    }

    /// Returns an iterator over the diffs in this batch, in the order they were added.
    ///
    /// The iterator yields tuples of `(&ContainerID, &Diff)` where:
    /// - `ContainerID` is the ID of the container that was modified
    /// - `Diff` contains the actual changes made to that container
    ///
    /// The order of the diffs is preserved from when they were originally added to the batch.
    pub fn get_diff(&self) -> Vec<ContainerIDAndDiff> {
        let batch = self.0.lock().unwrap();
        batch
            .iter()
            .map(|(id, diff)| ContainerIDAndDiff {
                cid: id.into(),
                diff: diff.into(),
            })
            .collect()
    }
}

impl From<DiffBatch> for loro::event::DiffBatch {
    fn from(value: DiffBatch) -> Self {
        value.0.lock().unwrap().clone()
    }
}

impl From<loro::event::DiffBatch> for DiffBatch {
    fn from(value: loro::event::DiffBatch) -> Self {
        Self(Arc::new(Mutex::new(value)))
    }
}

pub struct ContainerIDAndDiff {
    pub cid: ContainerID,
    pub diff: Diff,
}
