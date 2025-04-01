use crate::{
    container::richtext::richtext_state::{unicode_to_utf8_index, utf16_to_utf8_index},
    delta::{Delta, DeltaItem, Meta},
    event::{Diff, Index, Path, TextDiff, TextDiffItem, TextMeta},
    handler::ValueOrHandler,
    utils::string_slice::StringSlice,
};
use generic_btree::rle::HasLength;
use loro_common::ContainerType;
pub use loro_common::LoroValue;

// TODO: rename this trait
pub trait ToJson {
    fn to_json_value(&self) -> serde_json::Value;
    fn to_json(&self) -> String {
        self.to_json_value().to_string()
    }
    fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(&self.to_json_value()).unwrap()
    }
    fn from_json(s: &str) -> Self;
}

impl ToJson for LoroValue {
    fn to_json_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }

    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap()
    }

    #[allow(unused)]
    fn from_json(s: &str) -> Self {
        serde_json::from_str(s).unwrap()
    }
}

impl ToJson for DeltaItem<StringSlice, TextMeta> {
    fn to_json_value(&self) -> serde_json::Value {
        match self {
            Self::Retain {
                retain: len,
                attributes: meta,
            } => {
                let mut map = serde_json::Map::new();
                map.insert("retain".into(), serde_json::to_value(len).unwrap());
                if !meta.is_empty() {
                    map.insert("attributes".into(), meta.to_json_value());
                }
                serde_json::Value::Object(map)
            }
            Self::Insert {
                insert: value,
                attributes: meta,
            } => {
                let mut map = serde_json::Map::new();
                map.insert("insert".into(), serde_json::to_value(value).unwrap());
                if !meta.is_empty() {
                    map.insert("attributes".into(), meta.to_json_value());
                }
                serde_json::Value::Object(map)
            }
            Self::Delete {
                delete: len,
                attributes: _,
            } => {
                let mut map = serde_json::Map::new();
                map.insert("delete".into(), serde_json::to_value(len).unwrap());
                serde_json::Value::Object(map)
            }
        }
    }

    fn from_json(s: &str) -> Self {
        let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(s).unwrap();
        if map.contains_key("retain") {
            let len = map["retain"].as_u64().unwrap();
            let meta = if let Some(meta) = map.get("attributes") {
                TextMeta::from_json(meta.to_string().as_str())
            } else {
                TextMeta::default()
            };
            Self::Retain {
                retain: len as usize,
                attributes: meta,
            }
        } else if map.contains_key("insert") {
            let value = map["insert"].as_str().unwrap().to_string().into();
            let meta = if let Some(meta) = map.get("attributes") {
                TextMeta::from_json(meta.to_string().as_str())
            } else {
                TextMeta::default()
            };
            Self::Insert {
                insert: value,
                attributes: meta,
            }
        } else if map.contains_key("delete") {
            let len = map["delete"].as_u64().unwrap();
            Self::Delete {
                delete: len as usize,
                attributes: Default::default(),
            }
        } else {
            panic!("Invalid delta item: {}", s);
        }
    }
}

fn diff_item_to_json_value(item: &TextDiffItem) -> (serde_json::Value, Option<serde_json::Value>) {
    match item {
        loro_delta::DeltaItem::Retain { len, attr } => {
            let mut map = serde_json::Map::new();
            map.insert("retain".into(), serde_json::to_value(len).unwrap());
            if !attr.is_empty() {
                map.insert("attributes".into(), attr.to_json_value());
            }
            (serde_json::Value::Object(map), None)
        }
        loro_delta::DeltaItem::Replace {
            value,
            attr,
            delete,
        } => {
            let mut a = None;
            let mut b = None;
            if value.rle_len() > 0 {
                let mut map = serde_json::Map::new();
                map.insert("insert".into(), serde_json::to_value(value).unwrap());
                if !attr.is_empty() {
                    map.insert("attributes".into(), attr.to_json_value());
                }
                a = Some(serde_json::Value::Object(map));
            }
            if *delete > 0 {
                let mut map = serde_json::Map::new();
                map.insert("delete".into(), serde_json::to_value(delete).unwrap());
                b = Some(serde_json::Value::Object(map));
            }

            if a.is_none() {
                a = std::mem::take(&mut b);
                if a.is_none() {
                    let mut map = serde_json::Map::new();
                    map.insert("retain".into(), serde_json::to_value(0).unwrap());
                    a = Some(serde_json::Value::Object(map));
                }
            }
            (a.unwrap(), b)
        }
    }
}

fn diff_item_from_json(v: serde_json::Value) -> TextDiffItem {
    let serde_json::Value::Object(map) = v else {
        panic!("Invalid delta item: {:?}", v);
    };
    if map.contains_key("retain") {
        let len = map["retain"].as_u64().unwrap();
        let meta = if let Some(meta) = map.get("attributes") {
            TextMeta::from_json(meta.to_string().as_str())
        } else {
            TextMeta::default()
        };
        TextDiffItem::Retain {
            len: len as usize,
            attr: meta,
        }
    } else if map.contains_key("insert") {
        let value = map["insert"].as_str().unwrap().to_string().into();
        let meta = if let Some(meta) = map.get("attributes") {
            TextMeta::from_json(meta.to_string().as_str())
        } else {
            TextMeta::default()
        };
        TextDiffItem::Replace {
            value,
            attr: meta,
            delete: 0,
        }
    } else if map.contains_key("delete") {
        let len = map["delete"].as_u64().unwrap();
        TextDiffItem::new_delete(len as usize)
    } else {
        panic!("Invalid delta item: {:?}", map);
    }
}

impl ToJson for TextDiff {
    fn to_json_value(&self) -> serde_json::Value {
        let mut vec = Vec::new();
        for item in self.iter() {
            let (a, b) = diff_item_to_json_value(item);
            vec.push(a);
            if let Some(b) = b {
                vec.push(b);
            }
        }
        serde_json::Value::Array(vec)
    }

    fn from_json(s: &str) -> Self {
        let vec: Vec<serde_json::Value> = serde_json::from_str(s).unwrap();
        let mut ans = Self::new();
        for item in vec.into_iter() {
            ans.push(diff_item_from_json(item));
        }
        ans
    }
}

impl ToJson for Delta<StringSlice, TextMeta> {
    fn to_json_value(&self) -> serde_json::Value {
        let mut vec = Vec::new();
        for item in self.iter() {
            vec.push(item.to_json_value());
        }
        serde_json::Value::Array(vec)
    }

    fn from_json(s: &str) -> Self {
        let vec: Vec<serde_json::Value> = serde_json::from_str(s).unwrap();
        let mut ans = Self::new();
        for item in vec.into_iter() {
            ans.push(DeltaItem::from_json(item.to_string().as_str()));
        }
        ans
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TypeHint {
    Map,
    Text,
    List,
    Tree,
    #[cfg(feature = "counter")]
    Counter,
}

pub trait ApplyDiff {
    fn apply_diff_shallow(&mut self, diff: &[Diff]);
    fn apply_diff(&mut self, diff: &[Diff]);
    fn apply(&mut self, path: &Path, diff: &[Diff]);
}

impl ApplyDiff for LoroValue {
    fn apply_diff_shallow(&mut self, diff: &[Diff]) {
        match self {
            Self::String(value) => {
                let mut s = value.to_string();
                for item in diff.iter() {
                    let delta = item.as_text().unwrap();
                    let mut index = 0;
                    for delta_item in delta.iter() {
                        match delta_item {
                            loro_delta::DeltaItem::Retain { len, attr: _ } => {
                                index += len;
                            }
                            loro_delta::DeltaItem::Replace {
                                value,
                                attr: _,
                                delete,
                            } => {
                                let (start, end) = if cfg!(feature = "wasm") {
                                    (
                                        utf16_to_utf8_index(&s, index).unwrap(),
                                        utf16_to_utf8_index(&s, index + *delete).unwrap(),
                                    )
                                } else {
                                    (
                                        unicode_to_utf8_index(&s, index).unwrap(),
                                        unicode_to_utf8_index(&s, index + *delete).unwrap(),
                                    )
                                };
                                s.replace_range(start..end, value.as_str());
                                index += value.len_bytes();
                            }
                        }
                    }
                }
                *value = s.into()
            }
            Self::List(seq) => {
                let is_tree = matches!(diff.first(), Some(Diff::Tree(_)));
                if !is_tree {
                    let seq = seq.make_mut();
                    for item in diff.iter() {
                        let delta = item.as_list().unwrap();
                        let mut index = 0;
                        for delta_item in delta.iter() {
                            match delta_item {
                                loro_delta::DeltaItem::Retain { len, attr: _ } => {
                                    index += len;
                                }
                                loro_delta::DeltaItem::Replace {
                                    value,
                                    attr: _,
                                    delete,
                                } => {
                                    let len = value.len();
                                    seq.splice(
                                        index..index + delete,
                                        value.iter().map(|x| x.to_value()),
                                    );
                                    index += len;
                                }
                            }
                        }
                    }
                } else {
                    // let seq = Arc::make_mut(seq);
                    // for item in diff.iter() {
                    //     match item {
                    //         Diff::Tree(tree) => {
                    //             let mut v = TreeValue(seq);
                    //             v.apply_diff(tree);
                    //         }
                    //         _ => unreachable!(),
                    //     }
                    // }
                    unimplemented!()
                }
            }
            Self::Map(map) => {
                for item in diff.iter() {
                    match item {
                        Diff::Map(diff) => {
                            let map = map.make_mut();
                            for (key, value) in diff.updated.iter() {
                                match &value.value {
                                    Some(value) => {
                                        map.insert(key.to_string(), value.to_value());
                                    }
                                    None => {
                                        map.remove(&key.to_string());
                                    }
                                }
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    fn apply_diff(&mut self, diff: &[Diff]) {
        match self {
            Self::String(value) => {
                let mut s = value.to_string();
                for item in diff.iter() {
                    let delta = item.as_text().unwrap();
                    let mut index = 0;
                    for delta_item in delta.iter() {
                        match delta_item {
                            loro_delta::DeltaItem::Retain { len, attr: _ } => {
                                index += len;
                            }
                            loro_delta::DeltaItem::Replace {
                                value,
                                attr: _,
                                delete,
                            } => {
                                s.replace_range(index..index + *delete, value.as_str());
                                index += value.len_bytes();
                            }
                        }
                    }
                }
                *value = s.into();
            }
            Self::List(seq) => {
                let is_tree = matches!(diff.first(), Some(Diff::Tree(_)));
                if !is_tree {
                    let seq = seq.make_mut();
                    for item in diff.iter() {
                        let delta = item.as_list().unwrap();
                        let mut index = 0;
                        for delta_item in delta.iter() {
                            match delta_item {
                                loro_delta::DeltaItem::Retain { len, .. } => {
                                    index += len;
                                }
                                loro_delta::DeltaItem::Replace {
                                    value,
                                    attr: _,
                                    delete,
                                } => {
                                    let value_iter = value.iter().map(unresolved_to_collection);
                                    seq.splice(index..index + *delete, value_iter);
                                    index += value.len();
                                }
                            }
                        }
                    }
                } else {
                    // let seq = Arc::make_mut(seq);
                    // for item in diff.iter() {
                    //     match item {
                    //         Diff::Tree(tree) => {
                    //             let mut v = TreeValue(seq);
                    //             v.apply_diff(tree);
                    //         }
                    //         _ => unreachable!(),
                    //     }
                    // }
                    unimplemented!()
                }
            }
            Self::Map(map) => {
                for item in diff.iter() {
                    match item {
                        Diff::Map(diff) => {
                            let map = map.make_mut();
                            for (key, value) in diff.updated.iter() {
                                match &value.value {
                                    Some(value) => {
                                        map.insert(
                                            key.to_string(),
                                            unresolved_to_collection(value),
                                        );
                                    }
                                    None => {
                                        map.remove(&key.to_string());
                                    }
                                }
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            }
            _ => unreachable!(),
        }
    }

    fn apply(&mut self, path: &Path, diff: &[Diff]) {
        if diff.is_empty() {
            return;
        }

        let hint = match diff[0] {
            Diff::List(_) => TypeHint::List,
            Diff::Text(_) => TypeHint::Text,
            Diff::Map(_) => TypeHint::Map,
            Diff::Tree(_) => TypeHint::Tree,
            #[cfg(feature = "counter")]
            Diff::Counter(_) => TypeHint::Counter,
            Diff::Unknown => unreachable!(),
        };
        let value = {
            let mut hints = Vec::with_capacity(path.len());
            for item in path.iter().skip(1) {
                match item {
                    Index::Key(_) => hints.push(TypeHint::Map),
                    Index::Seq(_) => hints.push(TypeHint::List),
                    Index::Node(_) => hints.push(TypeHint::Tree),
                }
            }

            hints.push(hint);
            let mut value: &mut Self = self;
            for (item, hint) in path.iter().zip(hints.iter()) {
                match item {
                    Index::Key(key) => {
                        let m = value.as_map_mut().unwrap();
                        let map = m.make_mut();
                        value = map.entry(key.to_string()).or_insert_with(|| match hint {
                            TypeHint::Map => Self::Map(Default::default()),
                            TypeHint::Text => Self::String(Default::default()),
                            TypeHint::List => Self::List(Default::default()),
                            TypeHint::Tree => Self::List(Default::default()),
                            #[cfg(feature = "counter")]
                            TypeHint::Counter => Self::Double(0.),
                        })
                    }
                    Index::Seq(index) => {
                        let l = value.as_list_mut().unwrap();
                        let list = l.make_mut();
                        value = list.get_mut(*index).unwrap();
                    }
                    Index::Node(tree_id) => {
                        let l = value.as_list_mut().unwrap();
                        let list = l.make_mut();
                        let Some(map) = list.iter_mut().find(|x| {
                            let id = x.as_map().unwrap().get("id").unwrap().as_string().unwrap();
                            id.as_ref() == tree_id.to_string()
                        }) else {
                            // delete node first
                            return;
                        };
                        let map_mut = map.as_map_mut().unwrap().make_mut();
                        let meta = map_mut.get_mut("meta").unwrap();
                        if meta.is_container() {
                            *meta = ContainerType::Map.default_value();
                        }
                        value = meta
                    }
                }
            }
            value
        };
        value.apply_diff(diff);
    }
}

pub(crate) fn unresolved_to_collection(v: &ValueOrHandler) -> LoroValue {
    match v {
        ValueOrHandler::Value(v) => v.clone(),
        ValueOrHandler::Handler(c) => c.c_type().default_value(),
    }
}

#[cfg(feature = "wasm")]
pub mod wasm {
    use crate::{
        delta::{Delta, DeltaItem, Meta, StyleMeta, TreeDiff, TreeDiffItem, TreeExternalDiff},
        event::{Index, TextDiff, TextDiffItem, TextMeta},
        utils::string_slice::StringSlice,
        TreeParentId,
    };
    use fractional_index::FractionalIndex;
    use generic_btree::rle::HasLength;
    use js_sys::{Array, Object};
    use loro_common::{LoroValue, TreeID};
    use wasm_bindgen::{JsCast, JsValue, __rt::IntoJsResult};

    impl From<Index> for JsValue {
        fn from(value: Index) -> Self {
            match value {
                Index::Key(key) => Self::from_str(&key),
                Index::Seq(num) => Self::from_f64(num as f64),
                Index::Node(node) => node.into(),
            }
        }
    }

    impl From<&TreeDiff> for JsValue {
        fn from(value: &TreeDiff) -> Self {
            let array = Array::new();
            for diff in value.diff.iter() {
                let obj = Object::new();
                js_sys::Reflect::set(&obj, &"target".into(), &diff.target.into()).unwrap();
                match &diff.action {
                    TreeExternalDiff::Create {
                        parent,
                        index,
                        position,
                    } => {
                        js_sys::Reflect::set(&obj, &"action".into(), &"create".into()).unwrap();
                        js_sys::Reflect::set(&obj, &"parent".into(), &Self::from(parent.tree_id()))
                            .unwrap();
                        js_sys::Reflect::set(&obj, &"index".into(), &(*index).into()).unwrap();
                        js_sys::Reflect::set(
                            &obj,
                            &"fractionalIndex".into(),
                            &position.to_string().into(),
                        )
                        .unwrap();
                    }
                    TreeExternalDiff::Delete {
                        old_parent,
                        old_index,
                    } => {
                        js_sys::Reflect::set(&obj, &"action".into(), &"delete".into()).unwrap();
                        js_sys::Reflect::set(
                            &obj,
                            &"oldParent".into(),
                            &Self::from(old_parent.tree_id()),
                        )
                        .unwrap();
                        js_sys::Reflect::set(&obj, &"oldIndex".into(), &(*old_index).into())
                            .unwrap();
                    }
                    TreeExternalDiff::Move {
                        parent,
                        index,
                        position,
                        old_parent,
                        old_index,
                    } => {
                        js_sys::Reflect::set(&obj, &"action".into(), &"move".into()).unwrap();
                        js_sys::Reflect::set(&obj, &"parent".into(), &Self::from(parent.tree_id()))
                            .unwrap();
                        js_sys::Reflect::set(&obj, &"index".into(), &(*index).into()).unwrap();
                        js_sys::Reflect::set(
                            &obj,
                            &"fractionalIndex".into(),
                            &position.to_string().into(),
                        )
                        .unwrap();
                        js_sys::Reflect::set(
                            &obj,
                            &"oldParent".into(),
                            &Self::from(old_parent.tree_id()),
                        )
                        .unwrap();
                        js_sys::Reflect::set(&obj, &"oldIndex".into(), &(*old_index).into())
                            .unwrap();
                    }
                }
                array.push(&obj);
            }
            array.into_js_result().unwrap()
        }
    }

    impl TryFrom<&JsValue> for TreeDiff {
        type Error = String;

        fn try_from(value: &JsValue) -> Result<Self, Self::Error> {
            if !value.is_array() {
                return Err("Expected an array".to_string());
            }

            let array = js_sys::Array::from(value);
            let mut diff = Vec::new();

            for i in 0..array.length() {
                let item = array.get(i);
                if !item.is_object() {
                    return Err(format!("Item at index {} is not an object", i));
                }

                let obj = js_sys::Object::from(item);
                let target = js_sys::Reflect::get(&obj, &"target".into())
                    .map_err(|e| format!("Failed to get target: {:?}", e))?;
                let target = TreeID::try_from(target)
                    .map_err(|e| format!("Failed to parse target: {:?}", e))?;

                let action = js_sys::Reflect::get(&obj, &"action".into())
                    .map_err(|e| format!("Failed to get action: {:?}", e))?;
                let action = action
                    .as_string()
                    .ok_or_else(|| "action is not a string".to_string())?;

                let action = match action.as_str() {
                    "create" => {
                        let parent = js_sys::Reflect::get(&obj, &"parent".into())
                            .map_err(|e| format!("Failed to get parent: {:?}", e))?;
                        let parent_id = if parent.is_null() || parent.is_undefined() {
                            None
                        } else {
                            Some(
                                TreeID::try_from(parent)
                                    .map_err(|e| format!("Failed to parse parent: {:?}", e))?,
                            )
                        };
                        let parent = TreeParentId::from(parent_id);
                        let index = js_sys::Reflect::get(&obj, &"index".into())
                            .map_err(|e| format!("Failed to get index: {:?}", e))?;
                        let index = index
                            .as_f64()
                            .ok_or_else(|| "index is not a number".to_string())?
                            as usize;

                        let position = js_sys::Reflect::get(&obj, &"fractionalIndex".into())
                            .map_err(|e| format!("Failed to get fractionalIndex: {:?}", e))?;
                        let position = position
                            .as_string()
                            .ok_or_else(|| "fractionalIndex is not a string".to_string())?;
                        let position = FractionalIndex::from_hex_string(position);

                        TreeExternalDiff::Create {
                            parent,
                            index,
                            position,
                        }
                    }
                    "move" => {
                        let parent = js_sys::Reflect::get(&obj, &"parent".into())
                            .map_err(|e| format!("Failed to get parent: {:?}", e))?;
                        let parent_id = if parent.is_null() || parent.is_undefined() {
                            None
                        } else {
                            Some(
                                TreeID::try_from(parent)
                                    .map_err(|e| format!("Failed to parse parent: {:?}", e))?,
                            )
                        };
                        let parent = TreeParentId::from(parent_id);

                        let index = js_sys::Reflect::get(&obj, &"index".into())
                            .map_err(|e| format!("Failed to get index: {:?}", e))?;
                        let index = index
                            .as_f64()
                            .ok_or_else(|| "index is not a number".to_string())?
                            as usize;

                        let position = js_sys::Reflect::get(&obj, &"fractionalIndex".into())
                            .map_err(|e| format!("Failed to get fractionalIndex: {:?}", e))?;
                        let position = position
                            .as_string()
                            .ok_or_else(|| "fractionalIndex is not a string".to_string())?;
                        let position = FractionalIndex::from_hex_string(position);

                        let old_parent = js_sys::Reflect::get(&obj, &"oldParent".into())
                            .map_err(|e| format!("Failed to get oldParent: {:?}", e))?;
                        let old_parent_id = if old_parent.is_null() || old_parent.is_undefined() {
                            None
                        } else {
                            Some(
                                TreeID::try_from(old_parent)
                                    .map_err(|e| format!("Failed to parse oldParent: {:?}", e))?,
                            )
                        };
                        let old_parent = TreeParentId::from(old_parent_id);

                        let old_index = js_sys::Reflect::get(&obj, &"oldIndex".into())
                            .map_err(|e| format!("Failed to get oldIndex: {:?}", e))?;
                        let old_index = old_index
                            .as_f64()
                            .ok_or_else(|| "oldIndex is not a number".to_string())?
                            as usize;

                        TreeExternalDiff::Move {
                            parent,
                            index,
                            position,
                            old_parent,
                            old_index,
                        }
                    }
                    "delete" => {
                        let old_parent = js_sys::Reflect::get(&obj, &"oldParent".into())
                            .map_err(|e| format!("Failed to get oldParent: {:?}", e))?;
                        let old_parent_id = if old_parent.is_null() || old_parent.is_undefined() {
                            None
                        } else {
                            Some(
                                TreeID::try_from(old_parent)
                                    .map_err(|e| format!("Failed to parse oldParent: {:?}", e))?,
                            )
                        };
                        let old_parent = TreeParentId::from(old_parent_id);

                        let old_index = js_sys::Reflect::get(&obj, &"oldIndex".into())
                            .map_err(|e| format!("Failed to get oldIndex: {:?}", e))?;
                        let old_index = old_index
                            .as_f64()
                            .ok_or_else(|| "oldIndex is not a number".to_string())?
                            as usize;

                        TreeExternalDiff::Delete {
                            old_parent,
                            old_index,
                        }
                    }
                    action => Err(format!("Unknown tree diff action: {}", action))?,
                };

                diff.push(TreeDiffItem { target, action });
            }

            Ok(Self { diff })
        }
    }

    impl From<&Delta<StringSlice, StyleMeta>> for JsValue {
        fn from(value: &Delta<StringSlice, StyleMeta>) -> Self {
            let arr = Array::new_with_length(value.len() as u32);
            for (i, v) in value.iter().enumerate() {
                arr.set(i as u32, Self::from(v.clone()));
            }

            arr.into_js_result().unwrap()
        }
    }

    impl From<DeltaItem<StringSlice, StyleMeta>> for JsValue {
        fn from(value: DeltaItem<StringSlice, StyleMeta>) -> Self {
            let obj = Object::new();
            match value {
                DeltaItem::Retain {
                    retain: len,
                    attributes: meta,
                } => {
                    js_sys::Reflect::set(
                        &obj,
                        &Self::from_str("retain"),
                        &Self::from_f64(len as f64),
                    )
                    .unwrap();
                    if !meta.is_empty() {
                        js_sys::Reflect::set(
                            &obj,
                            &Self::from_str("attributes"),
                            &Self::from(&meta),
                        )
                        .unwrap();
                    }
                }
                DeltaItem::Insert {
                    insert: value,
                    attributes: meta,
                } => {
                    js_sys::Reflect::set(
                        &obj,
                        &Self::from_str("insert"),
                        &Self::from_str(value.as_str()),
                    )
                    .unwrap();
                    if !meta.is_empty() {
                        js_sys::Reflect::set(
                            &obj,
                            &Self::from_str("attributes"),
                            &Self::from(&meta),
                        )
                        .unwrap();
                    }
                }
                DeltaItem::Delete {
                    delete: len,
                    attributes: _,
                } => {
                    js_sys::Reflect::set(
                        &obj,
                        &Self::from_str("delete"),
                        &Self::from_f64(len as f64),
                    )
                    .unwrap();
                }
            }

            obj.into_js_result().unwrap()
        }
    }

    pub fn text_diff_to_js_value(diff: &TextDiff) -> JsValue {
        let arr = Array::new();
        let mut i = 0;
        for v in diff.iter() {
            let (a, b) = text_diff_item_to_js_value(v);
            arr.set(i as u32, a);
            i += 1;
            if let Some(b) = b {
                arr.set(i as u32, b);
                i += 1;
            }
        }

        arr.into_js_result().unwrap()
    }

    fn text_diff_item_to_js_value(value: &TextDiffItem) -> (JsValue, Option<JsValue>) {
        match value {
            loro_delta::DeltaItem::Retain { len, attr } => {
                let obj = Object::new();
                js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str("retain"),
                    &JsValue::from_f64(*len as f64),
                )
                .unwrap();
                if !attr.is_empty() {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("attributes"),
                        &JsValue::from(attr),
                    )
                    .unwrap();
                }
                (obj.into_js_result().unwrap(), None)
            }
            loro_delta::DeltaItem::Replace {
                value,
                attr,
                delete,
            } => {
                let mut a = None;
                let mut b = None;
                if value.rle_len() > 0 {
                    let obj = Object::new();
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("insert"),
                        &JsValue::from_str(value.as_str()),
                    )
                    .unwrap();
                    if !attr.is_empty() {
                        js_sys::Reflect::set(
                            &obj,
                            &JsValue::from_str("attributes"),
                            &JsValue::from(attr),
                        )
                        .unwrap();
                    }
                    a = Some(obj.into_js_result().unwrap());
                }

                if *delete > 0 {
                    let obj = Object::new();
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("delete"),
                        &JsValue::from_f64(*delete as f64),
                    )
                    .unwrap();
                    b = Some(obj.into_js_result().unwrap());
                }

                if a.is_none() {
                    a = std::mem::take(&mut b);
                }

                (a.unwrap(), b)
            }
        }
    }

    impl From<&StyleMeta> for JsValue {
        fn from(value: &StyleMeta) -> Self {
            // TODO: refactor: should we extract the common code of ToJson and ToJsValue
            let obj = Object::new();
            for (key, style) in value.iter() {
                let value = Self::from(style.data);
                js_sys::Reflect::set(&obj, &Self::from_str(&key), &value).unwrap();
            }

            obj.into_js_result().unwrap()
        }
    }

    impl From<&TextMeta> for JsValue {
        fn from(value: &TextMeta) -> Self {
            let obj = Object::new();
            for (key, value) in value.0.iter() {
                js_sys::Reflect::set(&obj, &Self::from_str(key), &Self::from(value.clone()))
                    .unwrap();
            }

            obj.into_js_result().unwrap()
        }
    }

    impl TryFrom<&JsValue> for TextMeta {
        type Error = JsValue;

        fn try_from(value: &JsValue) -> Result<Self, Self::Error> {
            if value.is_null() || value.is_undefined() {
                return Ok(Self::default());
            }

            let obj = value.dyn_ref::<Object>().ok_or("Expected an object")?;
            let mut meta = Self::default();

            let entries = Object::entries(obj);
            for i in 0..entries.length() {
                let entry = entries.get(i);
                let entry_arr = entry.dyn_ref::<Array>().ok_or("Expected an array")?;
                let key = entry_arr
                    .get(0)
                    .as_string()
                    .ok_or("Expected a string key")?;
                let value = entry_arr.get(1);
                meta.0.insert(key, LoroValue::from(value));
            }

            Ok(meta)
        }
    }
}
