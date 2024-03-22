use std::fmt::Debug;
use std::ops::{Deref, DerefMut};

use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use loro::ID;
use loro::{
    event::{Diff, DiffEvent, ListDiffItem},
    ContainerType, Index, LoroDoc, LoroText, LoroValue, TreeExternalDiff, TreeID, ValueOrContainer,
};
#[derive(Debug, EnumAsInner)]
pub enum Value {
    Value(LoroValue),
    Container(ContainerTracker),
}

impl Value {
    pub fn empty_container(ty: ContainerType) -> Self {
        match ty {
            ContainerType::Map => Value::Container(ContainerTracker::Map(MapTracker::empty())),
            ContainerType::List => {
                Value::Container(ContainerTracker::List(ListTracker(Vec::new())))
            }
            ContainerType::MovableList => {
                Value::Container(ContainerTracker::MovableList(MovableListTracker::empty()))
            }
            ContainerType::Text => Value::Container(ContainerTracker::Text(TextTracker::empty())),
            ContainerType::Tree => {
                Value::Container(ContainerTracker::Tree(TreeTracker(Vec::new())))
            }
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
}

impl ContainerTracker {
    pub fn to_value(&self) -> LoroValue {
        match self {
            ContainerTracker::Map(map) => map.to_value(),
            ContainerTracker::List(list) => list.to_value(),
            ContainerTracker::MovableList(list) => list.to_value(),
            ContainerTracker::Text(text) => text.to_value(),
            ContainerTracker::Tree(tree) => tree.to_value(),
        }
    }
}

#[derive(Debug)]
pub struct MapTracker(FxHashMap<String, Value>);
impl Deref for MapTracker {
    type Target = FxHashMap<String, Value>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for MapTracker {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl ApplyDiff for MapTracker {
    fn empty() -> Self {
        MapTracker(FxHashMap::default())
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
                        self.insert(k.to_string(), Value::empty_container(c.get_type()));
                    }
                }
            } else {
                self.remove(*k);
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
pub struct ListTracker(Vec<Value>);
impl Deref for ListTracker {
    type Target = Vec<Value>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for ListTracker {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl ApplyDiff for ListTracker {
    fn empty() -> Self {
        ListTracker(Vec::new())
    }

    fn apply_diff(&mut self, diff: Diff) {
        let diff = diff.as_list().unwrap();
        let mut index = 0;
        for item in diff.iter() {
            match item {
                ListDiffItem::Retain { retain: len } => {
                    index += len;
                }
                ListDiffItem::Insert {
                    insert: value,
                    move_from: _,
                } => {
                    for v in value {
                        let value = match v {
                            ValueOrContainer::Container(c) => Value::empty_container(c.get_type()),
                            ValueOrContainer::Value(v) => Value::Value(v.clone()),
                        };
                        self.insert(index, value);
                        index += 1;
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
pub struct MovableListTracker(Vec<Value>);
impl Deref for MovableListTracker {
    type Target = Vec<Value>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for MovableListTracker {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl ApplyDiff for MovableListTracker {
    fn empty() -> Self {
        MovableListTracker(Vec::new())
    }

    fn apply_diff(&mut self, diff: Diff) {
        let diff = diff.as_list().unwrap();
        let mut index = 0;
        for item in diff.iter() {
            match item {
                ListDiffItem::Retain { retain: len } => {
                    index += len;
                }
                ListDiffItem::Insert {
                    insert: value,
                    move_from,
                } => {
                    for v in value {
                        let value = match v {
                            ValueOrContainer::Container(c) => Value::empty_container(c.get_type()),
                            ValueOrContainer::Value(v) => Value::Value(v.clone()),
                        };
                        self.insert(index, value);
                        index += 1;
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

pub struct TextTracker {
    _doc: LoroDoc,
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
    fn empty() -> Self {
        let doc = LoroDoc::new();
        let text = doc.get_text("text");
        TextTracker { _doc: doc, text }
    }

    fn apply_diff(&mut self, diff: Diff) {
        let deltas = diff.as_text().unwrap();
        self.text.apply_delta(deltas).unwrap();
    }

    fn to_value(&self) -> LoroValue {
        self.text.to_string().into()
    }
}
#[derive(Debug)]
pub struct TreeTracker(Vec<TreeNode>);

impl ApplyDiff for TreeTracker {
    fn empty() -> Self {
        TreeTracker(Vec::new())
    }

    fn apply_diff(&mut self, diff: Diff) {
        let diff = diff.as_tree().unwrap();
        for diff in &diff.diff {
            let target = diff.target;
            match &diff.action {
                TreeExternalDiff::Create(parent) => {
                    let node = TreeNode::new(target, *parent);
                    self.push(node);
                }
                TreeExternalDiff::Delete => {
                    self.retain(|node| node.id != target && node.parent != Some(target));
                }
                TreeExternalDiff::Move(parent) => {
                    let node = self.iter_mut().find(|node| node.id == target).unwrap();
                    node.parent = *parent;
                }
            }
        }
    }

    fn to_value(&self) -> LoroValue {
        let mut list: Vec<FxHashMap<_, _>> = Vec::new();
        for node in self.iter() {
            let mut map = FxHashMap::default();
            map.insert("id".to_string(), node.id.to_string().into());
            map.insert("meta".to_string(), node.meta.to_value());
            map.insert(
                "parent".to_string(),
                match node.parent {
                    Some(parent) => parent.to_string().into(),
                    None => LoroValue::Null,
                },
            );
            list.push(map);
        }
        // compare by peer and then counter
        list.sort_by_key(|x| {
            let id: ID = x
                .get("id")
                .unwrap()
                .as_string()
                .unwrap()
                .as_str()
                .try_into()
                .unwrap();
            id
        });
        list.into()
    }
}

impl Deref for TreeTracker {
    type Target = Vec<TreeNode>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl DerefMut for TreeTracker {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug)]
pub struct TreeNode {
    id: TreeID,
    meta: ContainerTracker,
    parent: Option<TreeID>,
}

impl TreeNode {
    pub fn new(id: TreeID, parent: Option<TreeID>) -> Self {
        TreeNode {
            id,
            meta: ContainerTracker::Map(MapTracker::empty()),
            parent,
        }
    }
}

impl ContainerTracker {
    pub fn apply_diff(&mut self, diff: DiffEvent) {
        for diff in diff.events {
            let path = diff.path;
            let mut value: &mut ContainerTracker = self;
            for (_, index) in path {
                match index {
                    Index::Key(key) => {
                        value = value
                            .as_map_mut()
                            .unwrap()
                            .get_mut(&key.to_string())
                            .unwrap()
                            .as_container_mut()
                            .unwrap()
                    }
                    Index::Node(tree_id) => {
                        value = &mut value
                            .as_tree_mut()
                            .unwrap()
                            .iter_mut()
                            .find(|node| &node.id == tree_id)
                            .unwrap()
                            .meta
                    }
                    Index::Seq(idx) => {
                        value = value
                            .as_list_mut()
                            .unwrap()
                            .get_mut(*idx)
                            .unwrap()
                            .as_container_mut()
                            .unwrap()
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
            }
        }
    }
}

pub trait ApplyDiff {
    fn empty() -> Self;
    fn apply_diff(&mut self, diff: Diff);
    fn to_value(&self) -> LoroValue;
}
