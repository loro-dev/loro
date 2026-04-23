use std::fmt::Debug;
use std::ops::{Deref, DerefMut};

use enum_as_inner::EnumAsInner;
use loro::{
    event::{Diff, DiffEvent, ListDiffItem},
    ContainerType, Index, LoroDoc, LoroText, LoroValue, ValueOrContainer,
};
use rustc_hash::FxHashMap;

use crate::container::TreeTracker;
use loro::{ContainerID, ContainerTrait};

use crate::container::CounterTracker;

#[derive(Debug, EnumAsInner)]
pub enum Value {
    Value(LoroValue),
    Container(ContainerTracker),
}

impl Value {
    pub fn empty_container(ty: ContainerType, id: ContainerID) -> Self {
        match ty {
            ContainerType::Map => Value::Container(ContainerTracker::Map(MapTracker::empty(id))),
            ContainerType::List => Value::Container(ContainerTracker::List(ListTracker::empty(id))),
            ContainerType::MovableList => {
                Value::Container(ContainerTracker::MovableList(MovableListTracker::empty(id)))
            }
            ContainerType::Text => Value::Container(ContainerTracker::Text(TextTracker::empty(id))),
            ContainerType::Tree => Value::Container(ContainerTracker::Tree(TreeTracker::empty(id))),
            ContainerType::Counter => {
                Value::Container(ContainerTracker::Counter(CounterTracker::empty(id)))
            }
            ContainerType::Unknown(_) => unreachable!(),
        }
    }
}

impl From<LoroValue> for Value {
    fn from(value: LoroValue) -> Self {
        Value::Value(value)
    }
}

impl From<ContainerTracker> for Value {
    fn from(value: ContainerTracker) -> Self {
        Value::Container(value)
    }
}

#[derive(Debug, EnumAsInner)]
pub enum ContainerTracker {
    Map(MapTracker),
    List(ListTracker),
    MovableList(MovableListTracker),
    Text(TextTracker),
    Tree(TreeTracker),
    Counter(CounterTracker),
}

impl ContainerTracker {
    pub fn to_value(&self) -> LoroValue {
        match self {
            ContainerTracker::Map(map) => map.to_value(),
            ContainerTracker::List(list) => list.to_value(),
            ContainerTracker::MovableList(list) => list.to_value(),
            ContainerTracker::Text(text) => text.to_value(),
            ContainerTracker::Tree(tree) => tree.to_value(),
            ContainerTracker::Counter(counter) => counter.to_value(),
        }
    }

    fn type_name(&self) -> &'static str {
        match self {
            ContainerTracker::Map(_) => "Map",
            ContainerTracker::List(_) => "List",
            ContainerTracker::MovableList(_) => "MovableList",
            ContainerTracker::Text(_) => "Text",
            ContainerTracker::Tree(_) => "Tree",
            ContainerTracker::Counter(_) => "Counter",
        }
    }

    pub fn id(&self) -> &ContainerID {
        match self {
            ContainerTracker::Map(map) => map.id(),
            ContainerTracker::List(list) => list.id(),
            ContainerTracker::MovableList(list) => list.id(),
            ContainerTracker::Text(text) => text.id(),
            ContainerTracker::Tree(tree) => tree.id(),
            ContainerTracker::Counter(counter) => counter.id(),
        }
    }
}

#[derive(Debug)]
pub struct MapTracker {
    id: ContainerID,
    map: FxHashMap<String, Value>,
}
impl Deref for MapTracker {
    type Target = FxHashMap<String, Value>;
    fn deref(&self) -> &Self::Target {
        &self.map
    }
}
impl DerefMut for MapTracker {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.map
    }
}

impl ApplyDiff for MapTracker {
    fn empty(id: ContainerID) -> Self {
        MapTracker {
            map: Default::default(),
            id,
        }
    }

    fn id(&self) -> &ContainerID {
        &self.id
    }

    fn apply_diff(&mut self, diff: Diff) {
        let diff = diff.as_map().unwrap();
        for (k, v) in diff.updated.iter() {
            if let Some(v) = v {
                match v {
                    ValueOrContainer::Value(v) => {
                        self.insert(k.to_string(), v.clone().into());
                    }
                    ValueOrContainer::Container(c) => {
                        self.insert(k.to_string(), Value::empty_container(c.get_type(), c.id()));
                    }
                }
            } else {
                self.remove(k.deref());
            }
        }
    }

    fn to_value(&self) -> LoroValue {
        let mut map = FxHashMap::default();
        for (k, v) in self.iter() {
            match v {
                Value::Container(c) => {
                    map.insert(k.clone(), c.to_value());
                }
                Value::Value(v) => {
                    map.insert(k.clone(), v.clone());
                }
            }
        }
        map.into()
    }
}
#[derive(Debug)]
pub struct ListTracker {
    id: ContainerID,
    list: Vec<Value>,
}
impl Deref for ListTracker {
    type Target = Vec<Value>;
    fn deref(&self) -> &Self::Target {
        &self.list
    }
}
impl DerefMut for ListTracker {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.list
    }
}

impl ApplyDiff for ListTracker {
    fn empty(id: ContainerID) -> Self {
        Self {
            list: Vec::new(),
            id,
        }
    }

    fn id(&self) -> &ContainerID {
        &self.id
    }

    fn apply_diff(&mut self, diff: Diff) {
        let diff = diff.as_list().unwrap();
        let mut index = 0;
        for item in diff.iter() {
            match item {
                ListDiffItem::Retain { retain: len } => {
                    index += len;
                }
                ListDiffItem::Insert { insert: value, .. } => {
                    for v in value {
                        let value = match v {
                            ValueOrContainer::Container(c) => {
                                Value::empty_container(c.get_type(), c.id())
                            }
                            ValueOrContainer::Value(v) => Value::Value(v.clone()),
                        };
                        let insert_index = index.min(self.len());
                        self.insert(insert_index, value);
                        index = insert_index + 1;
                    }
                }
                ListDiffItem::Delete { delete: len } => {
                    self.drain(index..index + *len);
                }
            }
        }
    }

    fn to_value(&self) -> LoroValue {
        self.iter()
            .map(|v| match v {
                Value::Container(c) => c.to_value(),
                Value::Value(v) => v.clone(),
            })
            .collect::<Vec<_>>()
            .into()
    }
}

#[derive(Debug)]
pub struct MovableListTracker {
    list: Vec<Value>,
    id: ContainerID,
}

impl Deref for MovableListTracker {
    type Target = Vec<Value>;
    fn deref(&self) -> &Self::Target {
        &self.list
    }
}
impl DerefMut for MovableListTracker {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.list
    }
}

impl ApplyDiff for MovableListTracker {
    fn empty(id: ContainerID) -> Self {
        Self {
            list: Vec::new(),
            id,
        }
    }

    fn id(&self) -> &ContainerID {
        &self.id
    }

    fn apply_diff(&mut self, diff: Diff) {
        let diff = diff.as_list().unwrap();
        let mut index = 0;
        let mut maybe_from_move = FxHashMap::default();
        let mut id_to_container = FxHashMap::default();
        for item in diff.iter() {
            match item {
                ListDiffItem::Retain { retain: len } => {
                    index += len;
                }
                ListDiffItem::Insert {
                    insert: value,
                    is_move,
                } => {
                    for v in value {
                        let value = match v {
                            ValueOrContainer::Container(c) => {
                                if let Some(c) = id_to_container.remove(&c.id()) {
                                    Value::Container(c)
                                } else {
                                    if *is_move {
                                        maybe_from_move.insert(c.id().clone(), index);
                                    }
                                    Value::empty_container(c.get_type(), c.id())
                                }
                            }
                            ValueOrContainer::Value(v) => Value::Value(v.clone()),
                        };
                        let insert_index = index.min(self.len());
                        self.insert(insert_index, value);
                        index = insert_index + 1;
                    }
                }
                ListDiffItem::Delete { delete: len } => {
                    for v in self.drain(index..index + *len) {
                        if let Value::Container(c) = v {
                            let id = c.id().clone();
                            id_to_container.insert(id, c);
                        }
                    }
                }
            }
        }

        for (id, index) in maybe_from_move {
            if let Some(old) = id_to_container.remove(&id) {
                self.list[index] = Value::Container(old);
            } else {
                // It may happen that the container is moved and also the value changed
                // thus the container is not in the id_to_container map
            }
        }
    }

    fn to_value(&self) -> LoroValue {
        self.iter()
            .map(|v| match v {
                Value::Container(c) => c.to_value(),
                Value::Value(v) => v.clone(),
            })
            .collect::<Vec<_>>()
            .into()
    }
}

pub struct TextTracker {
    _doc: LoroDoc,
    id: ContainerID,
    pub text: LoroText,
}

impl Debug for TextTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextTracker")
            .field("text", &self.text)
            .finish()
    }
}

impl ApplyDiff for TextTracker {
    fn empty(id: ContainerID) -> Self {
        let doc = LoroDoc::new();
        let text = doc.get_text("text");
        TextTracker {
            _doc: doc,
            text,
            id,
        }
    }

    fn id(&self) -> &ContainerID {
        &self.id
    }

    fn apply_diff(&mut self, diff: Diff) {
        let deltas = diff.as_text().unwrap();
        self.text.apply_delta(deltas).unwrap();
    }

    fn to_value(&self) -> LoroValue {
        self.text.to_string().into()
    }
}

impl ContainerTracker {
    pub fn apply_diff(&mut self, diff: DiffEvent) {
        for diff in diff.events {
            let path = diff.path;
            let mut value: &mut ContainerTracker = self;
            for (_, index) in path {
                let type_name = value.type_name();
                match index {
                    Index::Key(key) => {
                        let Some(map) = value.as_map_mut() else {
                            tracing::warn!(
                                "apply_diff: expected Map at key {}, got {}",
                                key,
                                type_name
                            );
                            return;
                        };
                        let Some(v) = map.get_mut(&key.to_string()) else {
                            tracing::warn!("apply_diff: key {} not found in map", key);
                            return;
                        };
                        let Some(container) = v.as_container_mut() else {
                            tracing::warn!("apply_diff: key {} is not a container", key);
                            return;
                        };
                        value = container;
                    }
                    Index::Node(tree_id) => {
                        let Some(tree) = value.as_tree_mut() else {
                            tracing::warn!(
                                "apply_diff: expected Tree at node {:?}, got {}",
                                tree_id,
                                type_name
                            );
                            return;
                        };
                        let Some(node) = tree.find_node_by_id_mut(*tree_id) else {
                            tracing::warn!("apply_diff: tree node {:?} not found", tree_id);
                            return;
                        };
                        value = &mut node.meta;
                    }
                    Index::Seq(idx) => {
                        value = match value {
                            ContainerTracker::List(l) => {
                                let len = l.len();
                                let Some(item) = l.get_mut(*idx) else {
                                    tracing::warn!(
                                        "apply_diff: list index {} out of bounds (len={})",
                                        idx,
                                        len
                                    );
                                    return;
                                };
                                let Some(container) = item.as_container_mut() else {
                                    tracing::warn!(
                                        "apply_diff: list item at {} is not a container",
                                        idx
                                    );
                                    return;
                                };
                                container
                            }
                            ContainerTracker::MovableList(l) => {
                                let len = l.len();
                                let Some(item) = l.get_mut(*idx) else {
                                    tracing::warn!(
                                        "apply_diff: movable_list index {} out of bounds (len={})",
                                        idx,
                                        len
                                    );
                                    return;
                                };
                                let Some(container) = item.as_container_mut() else {
                                    tracing::warn!(
                                        "apply_diff: movable_list item at {} is not a container",
                                        idx
                                    );
                                    return;
                                };
                                container
                            }
                            _ => {
                                tracing::warn!(
                                    "apply_diff: expected List/MovableList at seq index {}, got {}",
                                    idx,
                                    type_name
                                );
                                return;
                            }
                        }
                    }
                }
            }
            let target = diff.target;
            let diff = diff.diff;
            match target.container_type() {
                ContainerType::Map => {
                    value.as_map_mut().unwrap().apply_diff(diff);
                }
                ContainerType::List => {
                    value.as_list_mut().unwrap().apply_diff(diff);
                }
                ContainerType::MovableList => {
                    value.as_movable_list_mut().unwrap().apply_diff(diff);
                }
                ContainerType::Text => {
                    value.as_text_mut().unwrap().apply_diff(diff);
                }
                ContainerType::Tree => {
                    value.as_tree_mut().unwrap().apply_diff(diff);
                }
                ContainerType::Counter => {
                    value.as_counter_mut().unwrap().apply_diff(diff);
                }
                ContainerType::Unknown(_) => unreachable!(),
            }
        }
    }
}

pub trait ApplyDiff {
    fn empty(id: ContainerID) -> Self;
    fn id(&self) -> &ContainerID;
    fn apply_diff(&mut self, diff: Diff);
    fn to_value(&self) -> LoroValue;
}
