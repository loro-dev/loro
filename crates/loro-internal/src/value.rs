use std::sync::Arc;

use crate::{
    delta::DeltaItem,
    event::{Diff, Index, Path},
};

use debug_log::debug_dbg;
pub use loro_common::LoroValue;

pub trait ToJson {
    fn to_json(&self) -> String;
    fn to_json_pretty(&self) -> String;
    fn from_json(s: &str) -> Self;
}

impl ToJson for LoroValue {
    fn to_json(&self) -> String {
        #[cfg(feature = "json")]
        let ans = serde_json::to_string(self).unwrap();
        #[cfg(not(feature = "json"))]
        let ans = String::new();
        ans
    }

    fn to_json_pretty(&self) -> String {
        #[cfg(feature = "json")]
        let ans = serde_json::to_string_pretty(self).unwrap();
        #[cfg(not(feature = "json"))]
        let ans = String::new();
        ans
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

#[derive(Debug, PartialEq, Eq)]
enum TypeHint {
    Map,
    Text,
    List,
    Richtext,
}

pub trait ApplyDiff {
    fn apply_diff(&mut self, diff: &[Diff]);
    fn apply(&mut self, path: &Path, diff: &[Diff]);
}

impl ApplyDiff for LoroValue {
    fn apply_diff(&mut self, diff: &[Diff]) {
        match self {
            LoroValue::String(value) => {
                let mut s = value.to_string();
                for item in diff.iter() {
                    let delta = item.as_text().unwrap();
                    let mut index = 0;
                    for delta_item in delta.iter() {
                        match delta_item {
                            DeltaItem::Retain { len, .. } => {
                                index += len;
                            }
                            DeltaItem::Insert { value, .. } => {
                                s.insert_str(index, value);
                                index += value.len();
                            }
                            DeltaItem::Delete { len, .. } => {
                                s.drain(index..index + len);
                            }
                        }
                    }
                }
                *value = Arc::new(s);
            }
            LoroValue::List(seq) => {
                let seq = Arc::make_mut(seq);
                for item in diff.iter() {
                    let delta = item.as_list().unwrap();
                    let mut index = 0;
                    for delta_item in delta.iter() {
                        match delta_item {
                            DeltaItem::Retain { len, .. } => {
                                index += len;
                            }
                            DeltaItem::Insert { value, .. } => {
                                value.iter().for_each(|v| {
                                    let value = unresolved_to_collection(v);
                                    seq.insert(index, value);
                                    index += 1;
                                });
                            }
                            DeltaItem::Delete { len, .. } => {
                                seq.drain(index..index + len);
                            }
                        }
                    }
                }
            }
            LoroValue::Map(map) => {
                for item in diff.iter() {
                    match item {
                        Diff::Map(diff) => {
                            let map = Arc::make_mut(map);
                            for v in diff.added.iter() {
                                map.insert(v.0.to_string(), unresolved_to_collection(v.1));
                            }
                            for (k, _) in diff.deleted.iter() {
                                // map.remove(v.as_ref());
                                map.insert(k.to_string(), LoroValue::Null);
                            }
                            for (key, value) in diff.updated.iter() {
                                map.insert(key.to_string(), unresolved_to_collection(&value.new));
                            }
                        }
                        Diff::NewMap(diff) => {
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

        debug_dbg!(&self);
    }

    fn apply(&mut self, path: &Path, diff: &[Diff]) {
        if diff.is_empty() {
            return;
        }

        let hint = match diff[0] {
            Diff::List(_) => TypeHint::List,
            Diff::Text(_) => TypeHint::Text,
            Diff::Map(_) => TypeHint::Map,
            Diff::NewMap(_) => TypeHint::Map,
            Diff::SeqRaw(_) => TypeHint::Text,
            Diff::SeqRawUtf16(_) => TypeHint::Text,
            Diff::RichtextRaw(_) => TypeHint::Richtext,
        };
        {
            let mut hints = Vec::with_capacity(path.len());
            for item in path.iter().skip(1) {
                match item {
                    Index::Key(_) => hints.push(TypeHint::Map),
                    Index::Seq(_) => hints.push(TypeHint::List),
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
                            TypeHint::Text => LoroValue::String(Arc::new(String::new())),
                            TypeHint::List => LoroValue::List(Default::default()),
                            TypeHint::Richtext => LoroValue::List(Default::default()),
                        })
                    }
                    Index::Seq(index) => {
                        let l = value.as_list_mut().unwrap();
                        let list = Arc::make_mut(l);
                        value = list.get_mut(*index).unwrap();
                    }
                }
            }

            value
        }
        .apply_diff(diff);
    }
}

fn unresolved_to_collection(v: &LoroValue) -> LoroValue {
    if let Some(container) = v.as_container() {
        match container.container_type() {
            crate::ContainerType::Text => LoroValue::String(Default::default()),
            crate::ContainerType::Map => LoroValue::Map(Default::default()),
            crate::ContainerType::List => LoroValue::List(Default::default()),
        }
    } else {
        v.clone()
    }
}

#[cfg(feature = "wasm")]
pub mod wasm {
    use std::sync::Arc;

    use js_sys::{Array, Object, Uint8Array};
    use wasm_bindgen::{JsValue, __rt::IntoJsResult};

    use crate::{
        delta::{Delta, DeltaItem, MapDelta, MapDiff},
        event::{Diff, Index},
        LoroValue,
    };

    impl From<Index> for JsValue {
        fn from(value: Index) -> Self {
            match value {
                Index::Key(key) => JsValue::from_str(&key),
                Index::Seq(num) => JsValue::from_f64(num as f64),
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
                    js_sys::Reflect::set(&obj, &JsValue::from_str("diff"), &text.into()).unwrap();
                }
                Diff::Map(map) => {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("map"),
                    )
                    .unwrap();

                    js_sys::Reflect::set(&obj, &JsValue::from_str("diff"), &map.into()).unwrap();
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
                Diff::SeqRaw(text) => {
                    // set type as "text"
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("seq_raw"),
                    )
                    .unwrap();
                    // set diff as array
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("diff"),
                        &serde_wasm_bindgen::to_value(&text).unwrap(),
                    )
                    .unwrap();
                }
                Diff::SeqRawUtf16(text) => {
                    // set type as "text"
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("seq_raw_utf16"),
                    )
                    .unwrap();
                    // set diff as array
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("diff"),
                        &serde_wasm_bindgen::to_value(&text).unwrap(),
                    )
                    .unwrap();
                }
                Diff::RichtextRaw(_) => todo!(),
            };

            // convert object to js value
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

    impl From<Delta<String>> for JsValue {
        fn from(value: Delta<String>) -> Self {
            let arr = Array::new_with_length(value.len() as u32);
            for (i, v) in value.iter().enumerate() {
                arr.set(i as u32, JsValue::from(v.clone()));
            }

            arr.into_js_result().unwrap()
        }
    }

    impl From<DeltaItem<String, ()>> for JsValue {
        fn from(value: DeltaItem<String, ()>) -> Self {
            let obj = Object::new();
            match value {
                DeltaItem::Retain { len, meta: _ } => {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("retain"),
                    )
                    .unwrap();
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("len"),
                        &JsValue::from_f64(len as f64),
                    )
                    .unwrap();
                }
                DeltaItem::Insert { value, .. } => {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("insert"),
                    )
                    .unwrap();

                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("value"),
                        &JsValue::from_str(value.as_str()),
                    )
                    .unwrap();
                }
                DeltaItem::Delete { len, meta: _ } => {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("delete"),
                    )
                    .unwrap();
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("len"),
                        &JsValue::from_f64(len as f64),
                    )
                    .unwrap();
                }
            }

            obj.into_js_result().unwrap()
        }
    }

    impl From<DeltaItem<Vec<LoroValue>, ()>> for JsValue {
        fn from(value: DeltaItem<Vec<LoroValue>, ()>) -> Self {
            let obj = Object::new();
            match value {
                DeltaItem::Retain { len, .. } => {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("retain"),
                    )
                    .unwrap();
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("len"),
                        &JsValue::from_f64(len as f64),
                    )
                    .unwrap();
                }
                DeltaItem::Insert { value, .. } => {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("insert"),
                    )
                    .unwrap();

                    let arr = Array::new_with_length(value.len() as u32);
                    for (i, v) in value.into_iter().enumerate() {
                        arr.set(i as u32, convert(v));
                    }

                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("value"),
                        &arr.into_js_result().unwrap(),
                    )
                    .unwrap();
                }
                DeltaItem::Delete { len, .. } => {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("delete"),
                    )
                    .unwrap();
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("len"),
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
