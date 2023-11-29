use std::sync::Arc;

use crate::{
    delta::{Delta, DeltaItem, Meta, StyleMeta, TreeValue},
    event::{Index, Path, ResolvedDiff},
    handler::ValueOrContainer,
    utils::string_slice::StringSlice,
};

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
        #[cfg(feature = "json")]
        let ans = serde_json::from_str(s).unwrap();
        #[cfg(not(feature = "json"))]
        let ans = LoroValue::Null;
        ans
    }
}

impl ToJson for DeltaItem<StringSlice, StyleMeta> {
    fn to_json_value(&self) -> serde_json::Value {
        match self {
            DeltaItem::Retain {
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
            DeltaItem::Insert {
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
            DeltaItem::Delete {
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
                StyleMeta::from_json(meta.to_string().as_str())
            } else {
                StyleMeta::default()
            };
            DeltaItem::Retain {
                retain: len as usize,
                attributes: meta,
            }
        } else if map.contains_key("insert") {
            let value = map["insert"].as_str().unwrap().to_string().into();
            let meta = if let Some(meta) = map.get("attributes") {
                StyleMeta::from_json(meta.to_string().as_str())
            } else {
                StyleMeta::default()
            };
            DeltaItem::Insert {
                insert: value,
                attributes: meta,
            }
        } else if map.contains_key("delete") {
            let len = map["delete"].as_u64().unwrap();
            DeltaItem::Delete {
                delete: len as usize,
                attributes: Default::default(),
            }
        } else {
            panic!("Invalid delta item: {}", s);
        }
    }
}

impl ToJson for Delta<StringSlice, StyleMeta> {
    fn to_json_value(&self) -> serde_json::Value {
        let mut vec = Vec::new();
        for item in self.iter() {
            vec.push(item.to_json_value());
        }
        serde_json::Value::Array(vec)
    }

    fn from_json(s: &str) -> Self {
        let vec: Vec<serde_json::Value> = serde_json::from_str(s).unwrap();
        let mut ans = Delta::new();
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
}

pub trait ApplyDiff {
    fn apply_diff(&mut self, diff: &[ResolvedDiff]);
    fn apply(&mut self, path: &Path, diff: &[ResolvedDiff]);
}

impl ApplyDiff for LoroValue {
    fn apply_diff(&mut self, diff: &[ResolvedDiff]) {
        match self {
            LoroValue::String(value) => {
                let mut s = value.to_string();
                for item in diff.iter() {
                    let delta = item.as_text().unwrap();
                    let mut index = 0;
                    for delta_item in delta.iter() {
                        match delta_item {
                            DeltaItem::Retain { retain: len, .. } => {
                                index += len;
                            }
                            DeltaItem::Insert { insert: value, .. } => {
                                s.insert_str(index, value.as_str());
                                index += value.len_bytes();
                            }
                            DeltaItem::Delete { delete: len, .. } => {
                                s.drain(index..index + len);
                            }
                        }
                    }
                }
                *value = Arc::new(s);
            }
            LoroValue::List(seq) => {
                let is_tree = matches!(diff.first(), Some(ResolvedDiff::Tree(_)));
                if !is_tree {
                    let seq = Arc::make_mut(seq);
                    for item in diff.iter() {
                        let delta = item.as_list().unwrap();
                        let mut index = 0;
                        for delta_item in delta.iter() {
                            match delta_item {
                                DeltaItem::Retain { retain: len, .. } => {
                                    index += len;
                                }
                                DeltaItem::Insert { insert: value, .. } => {
                                    value.iter().for_each(|v| {
                                        let value = unresolved_to_collection(v);
                                        seq.insert(index, value);
                                        index += 1;
                                    });
                                }
                                DeltaItem::Delete { delete: len, .. } => {
                                    seq.drain(index..index + len);
                                }
                            }
                        }
                    }
                } else {
                    let seq = Arc::make_mut(seq);
                    for item in diff.iter() {
                        match item {
                            ResolvedDiff::Tree(tree) => {
                                let mut v = TreeValue(seq);
                                v.apply_diff(tree);
                            }
                            _ => unreachable!(),
                        }
                    }
                }
            }
            LoroValue::Map(map) => {
                for item in diff.iter() {
                    match item {
                        ResolvedDiff::NewMap(diff) => {
                            let map = Arc::make_mut(map);
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

    fn apply(&mut self, path: &Path, diff: &[ResolvedDiff]) {
        if diff.is_empty() {
            return;
        }

        let hint = match diff[0] {
            ResolvedDiff::List(_) => TypeHint::List,
            ResolvedDiff::Text(_) => TypeHint::Text,
            ResolvedDiff::NewMap(_) => TypeHint::Map,
            ResolvedDiff::Tree(_) => TypeHint::Tree,
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
            let mut value: &mut LoroValue = self;
            for (item, hint) in path.iter().zip(hints.iter()) {
                match item {
                    Index::Key(key) => {
                        let m = value.as_map_mut().unwrap();
                        let map = Arc::make_mut(m);
                        value = map.entry(key.to_string()).or_insert_with(|| match hint {
                            TypeHint::Map => LoroValue::Map(Default::default()),
                            TypeHint::Text => LoroValue::String(Default::default()),
                            TypeHint::List => LoroValue::List(Default::default()),
                            TypeHint::Tree => LoroValue::List(Default::default()),
                        })
                    }
                    Index::Seq(index) => {
                        let l = value.as_list_mut().unwrap();
                        let list = Arc::make_mut(l);
                        value = list.get_mut(*index).unwrap();
                    }
                    Index::Node(tree_id) => {
                        let l = value.as_list_mut().unwrap();
                        let list = Arc::make_mut(l);
                        let Some(map) = list.iter_mut().find(|x| {
                            let id = x.as_map().unwrap().get("id").unwrap().as_string().unwrap();
                            id.as_ref() == &tree_id.to_string()
                        }) else {
                            // delete node first
                            return;
                        };
                        let map_mut = Arc::make_mut(map.as_map_mut().unwrap());
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

pub(crate) fn unresolved_to_collection(v: &ValueOrContainer) -> LoroValue {
    match v {
        ValueOrContainer::Value(v) => v.clone(),
        ValueOrContainer::Container(c) => c.c_type().default_value(),
    }
}

#[cfg(feature = "wasm")]
pub mod wasm {
    use std::sync::Arc;

    use js_sys::{Array, Object, Uint8Array};
    use wasm_bindgen::{JsValue, __rt::IntoJsResult};

    use crate::{
        delta::{Delta, DeltaItem, MapDelta, MapDiff, Meta, StyleMeta, TreeDiff, TreeExternalDiff},
        event::{Diff, Index},
        utils::string_slice::StringSlice,
        LoroValue,
    };

    impl From<Index> for JsValue {
        fn from(value: Index) -> Self {
            match value {
                Index::Key(key) => JsValue::from_str(&key),
                Index::Seq(num) => JsValue::from_f64(num as f64),
                Index::Node(node) => JsValue::from_str(&node.to_string()),
            }
        }
    }

    pub fn convert(value: LoroValue) -> JsValue {
        match value {
            LoroValue::Null => JsValue::NULL,
            LoroValue::Bool(b) => JsValue::from_bool(b),
            LoroValue::Double(f) => JsValue::from_f64(f),
            LoroValue::I32(i) => JsValue::from_f64(i as f64),
            LoroValue::String(s) => JsValue::from_str(&s),
            LoroValue::List(list) => {
                let list = Arc::try_unwrap(list).unwrap_or_else(|m| (*m).clone());
                let arr = Array::new_with_length(list.len() as u32);
                for (i, v) in list.into_iter().enumerate() {
                    arr.set(i as u32, convert(v));
                }
                arr.into_js_result().unwrap()
            }
            LoroValue::Map(m) => {
                let m = Arc::try_unwrap(m).unwrap_or_else(|m| (*m).clone());
                let map = Object::new();
                for (k, v) in m.into_iter() {
                    let str: &str = &k;
                    js_sys::Reflect::set(&map, &JsValue::from_str(str), &convert(v)).unwrap();
                }

                map.into_js_result().unwrap()
            }
            LoroValue::Container(container_id) => JsValue::from(container_id),
            LoroValue::Binary(binary) => {
                let binary = Arc::try_unwrap(binary).unwrap_or_else(|m| (*m).clone());
                let arr = Uint8Array::new_with_length(binary.len() as u32);
                for (i, v) in binary.into_iter().enumerate() {
                    arr.set_index(i as u32, v);
                }
                arr.into_js_result().unwrap()
            }
        }
    }

    impl From<Diff> for JsValue {
        fn from(value: Diff) -> Self {
            // create a obj
            let obj = Object::new();
            match value {
                Diff::Tree(tree) => {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("tree"),
                    )
                    .unwrap();

                    js_sys::Reflect::set(&obj, &JsValue::from_str("diff"), &tree.into()).unwrap();
                }
                Diff::List(list) => {
                    // set type as "list"
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("list"),
                    )
                    .unwrap();
                    // set diff as array
                    let arr = Array::new_with_length(list.len() as u32);
                    for (i, v) in list.iter().enumerate() {
                        arr.set(i as u32, JsValue::from(v.clone()));
                    }
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("diff"),
                        &arr.into_js_result().unwrap(),
                    )
                    .unwrap();
                }
                Diff::Text(text) => {
                    // set type as "text"
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("text"),
                    )
                    .unwrap();
                    // set diff as array
                    js_sys::Reflect::set(&obj, &JsValue::from_str("diff"), &JsValue::from(text))
                        .unwrap();
                }
                Diff::NewMap(map) => {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("map"),
                    )
                    .unwrap();

                    js_sys::Reflect::set(&obj, &JsValue::from_str("updated"), &map.into()).unwrap();
                }
            };

            // convert object to js value
            obj.into_js_result().unwrap()
        }
    }

    impl From<TreeExternalDiff> for JsValue {
        fn from(value: TreeExternalDiff) -> Self {
            let obj = Object::new();
            match value {
                TreeExternalDiff::Delete => {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("delete"),
                    )
                    .unwrap();
                }
                TreeExternalDiff::Move(parent) => {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("move"),
                    )
                    .unwrap();

                    js_sys::Reflect::set(&obj, &JsValue::from_str("parent"), &parent.into())
                        .unwrap();
                }

                TreeExternalDiff::Create => {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("create"),
                    )
                    .unwrap();
                }
            }
            obj.into_js_result().unwrap()
        }
    }

    impl From<TreeDiff> for JsValue {
        fn from(value: TreeDiff) -> Self {
            let obj = Object::new();
            for diff in value.diff.into_iter() {
                js_sys::Reflect::set(&obj, &"target".into(), &diff.target.into()).unwrap();
                js_sys::Reflect::set(&obj, &"action".into(), &diff.action.into()).unwrap();
            }
            obj.into_js_result().unwrap()
        }
    }

    impl From<MapDelta> for JsValue {
        fn from(value: MapDelta) -> Self {
            let obj = Object::new();
            for (key, value) in value.updated.iter() {
                js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str(key),
                    &JsValue::from(value.value.clone()),
                )
                .unwrap();
            }

            obj.into_js_result().unwrap()
        }
    }

    impl From<MapDiff<LoroValue>> for JsValue {
        fn from(value: MapDiff<LoroValue>) -> Self {
            let obj = Object::new();
            {
                let added = Object::new();
                for (key, value) in value.added.iter() {
                    js_sys::Reflect::set(
                        &added,
                        &JsValue::from_str(key),
                        &JsValue::from(value.clone()),
                    )
                    .unwrap();
                }

                js_sys::Reflect::set(&obj, &JsValue::from_str("added"), &added).unwrap();
            }

            {
                let deleted = Object::new();
                for (key, value) in value.deleted.iter() {
                    js_sys::Reflect::set(
                        &deleted,
                        &JsValue::from_str(key),
                        &JsValue::from(value.clone()),
                    )
                    .unwrap();
                }

                js_sys::Reflect::set(&obj, &JsValue::from_str("deleted"), &deleted).unwrap();
            }

            {
                let updated = Object::new();
                for (key, pair) in value.updated.iter() {
                    let pair_obj = Object::new();
                    js_sys::Reflect::set(
                        &pair_obj,
                        &JsValue::from_str("old"),
                        &pair.old.clone().into(),
                    )
                    .unwrap();
                    js_sys::Reflect::set(
                        &pair_obj,
                        &JsValue::from_str("new"),
                        &pair.new.clone().into(),
                    )
                    .unwrap();
                    js_sys::Reflect::set(
                        &updated,
                        &JsValue::from_str(key),
                        &pair_obj.into_js_result().unwrap(),
                    )
                    .unwrap();
                }

                js_sys::Reflect::set(&obj, &JsValue::from_str("updated"), &updated).unwrap();
            }

            obj.into_js_result().unwrap()
        }
    }

    impl From<Delta<StringSlice, StyleMeta>> for JsValue {
        fn from(value: Delta<StringSlice, StyleMeta>) -> Self {
            let arr = Array::new_with_length(value.len() as u32);
            for (i, v) in value.iter().enumerate() {
                arr.set(i as u32, JsValue::from(v.clone()));
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
                        &JsValue::from_str("retain"),
                        &JsValue::from_f64(len as f64),
                    )
                    .unwrap();
                    if !meta.is_empty() {
                        js_sys::Reflect::set(
                            &obj,
                            &JsValue::from_str("attributes"),
                            &JsValue::from(meta),
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
                        &JsValue::from_str("insert"),
                        &JsValue::from_str(value.as_str()),
                    )
                    .unwrap();
                    if !meta.is_empty() {
                        js_sys::Reflect::set(
                            &obj,
                            &JsValue::from_str("attributes"),
                            &JsValue::from(meta),
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
                        &JsValue::from_str("delete"),
                        &JsValue::from_f64(len as f64),
                    )
                    .unwrap();
                }
            }

            obj.into_js_result().unwrap()
        }
    }

    impl From<StyleMeta> for JsValue {
        fn from(value: StyleMeta) -> Self {
            // TODO: refactor: should we extract the common code of ToJson and ToJsValue
            let obj = Object::new();
            for (key, style) in value.iter() {
                let value = if !key.contains_id() {
                    JsValue::from(style.data)
                } else {
                    let value = Object::new();
                    js_sys::Reflect::set(&value, &"key".into(), &JsValue::from_str(&style.key))
                        .unwrap();
                    let data = JsValue::from(style.data);
                    js_sys::Reflect::set(&value, &"data".into(), &data).unwrap();
                    value.into()
                };
                js_sys::Reflect::set(&obj, &JsValue::from_str(&key.to_attr_key()), &value).unwrap();
            }

            obj.into_js_result().unwrap()
        }
    }

    impl From<DeltaItem<Vec<LoroValue>, ()>> for JsValue {
        fn from(value: DeltaItem<Vec<LoroValue>, ()>) -> Self {
            let obj = Object::new();
            match value {
                DeltaItem::Retain { retain: len, .. } => {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("retain"),
                        &JsValue::from_f64(len as f64),
                    )
                    .unwrap();
                }
                DeltaItem::Insert { insert: value, .. } => {
                    let arr = Array::new_with_length(value.len() as u32);
                    for (i, v) in value.into_iter().enumerate() {
                        arr.set(i as u32, convert(v));
                    }

                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("insert"),
                        &arr.into_js_result().unwrap(),
                    )
                    .unwrap();
                }
                DeltaItem::Delete { delete: len, .. } => {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("delete"),
                        &JsValue::from_f64(len as f64),
                    )
                    .unwrap();
                }
            }

            obj.into_js_result().unwrap()
        }
    }
}

#[cfg(test)]
#[cfg(feature = "json")]
mod json_test {
    use crate::{fx_map, value::ToJson, LoroValue};

    #[test]
    fn list() {
        let list = LoroValue::List(
            vec![12.into(), "123".into(), fx_map!("kk" => 123.into()).into()].into(),
        );
        let json = list.to_json();
        println!("{}", json);
        assert_eq!(LoroValue::from_json(&json), list);
    }
}
