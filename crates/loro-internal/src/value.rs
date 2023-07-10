use std::{collections::HashMap, sync::Arc};

use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use serde::{de::VariantAccess, ser::SerializeStruct, Deserialize, Serialize};

use crate::{
    container::{registry::ContainerRegistry, ContainerID},
    delta::DeltaItem,
    event::{Diff, Index, Path},
    ContainerTrait,
};

/// [LoroValue] is used to represents the state of CRDT at a given version.
/// This struct is cheap to clone, the time complexity is O(1)
#[derive(Debug, PartialEq, Clone, EnumAsInner, Default)]
pub enum LoroValue {
    #[default]
    Null,
    Bool(bool),
    Double(f64),
    I32(i32),
    // i64?
    String(Arc<String>),
    List(Arc<Vec<LoroValue>>),
    Map(Arc<FxHashMap<String, LoroValue>>),
    Container(ContainerID),
}

#[derive(Serialize, Deserialize)]
enum Test {
    Unknown(ContainerID),
    Map(FxHashMap<u32, usize>),
}

impl LoroValue {
    pub(crate) fn resolve_deep(mut self, reg: &ContainerRegistry) -> LoroValue {
        match &mut self {
            LoroValue::List(list) => {
                let list = Arc::make_mut(list);
                for v in list.iter_mut() {
                    if v.as_container().is_some() {
                        *v = v.clone().resolve_deep(reg)
                    }
                }
            }
            LoroValue::Map(map) => {
                let map = Arc::make_mut(map);
                for v in map.values_mut() {
                    if v.as_container().is_some() {
                        *v = v.clone().resolve_deep(reg)
                    }
                }
            }
            LoroValue::Container(id) => {
                self = reg
                    .get(id)
                    .map(|container| {
                        let mut value =
                            container.upgrade().unwrap().try_lock().unwrap().get_value();

                        match &mut value {
                            LoroValue::List(list) => {
                                let list = Arc::make_mut(list);
                                for v in list.iter_mut() {
                                    if v.as_container().is_some() {
                                        *v = v.clone().resolve_deep(reg)
                                    }
                                }
                            }
                            LoroValue::Map(map) => {
                                let map = Arc::make_mut(map);
                                for v in map.values_mut() {
                                    if v.as_container().is_some() {
                                        *v = v.clone().resolve_deep(reg)
                                    }
                                }
                            }
                            LoroValue::Container(_) => unreachable!(),
                            _ => {}
                        }

                        value
                    })
                    .unwrap_or_else(|| match id.container_type() {
                        crate::ContainerType::Text => LoroValue::String(Default::default()),
                        crate::ContainerType::Map => LoroValue::Map(Default::default()),
                        crate::ContainerType::List => LoroValue::List(Default::default()),
                    })
            }
            _ => {}
        }
        self
    }

    #[cfg(feature = "json")]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    #[cfg(feature = "json")]
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap()
    }

    pub fn to_json_value(&self, reg: &ContainerRegistry) -> LoroValue {
        match self {
            LoroValue::Container(_) => self.clone().resolve_deep(reg).to_json_value(reg),
            _ => self.clone(),
        }
    }

    #[cfg(feature = "json")]
    pub fn from_json(s: &str) -> Self {
        serde_json::from_str(s).unwrap()
    }
}

impl<S: Into<String>, M> From<HashMap<S, LoroValue, M>> for LoroValue {
    fn from(map: HashMap<S, LoroValue, M>) -> Self {
        let mut new_map = FxHashMap::default();
        for (k, v) in map {
            new_map.insert(k.into(), v);
        }

        LoroValue::Map(Arc::new(new_map))
    }
}

impl<T: Into<LoroValue>> From<Vec<T>> for LoroValue {
    fn from(vec: Vec<T>) -> Self {
        LoroValue::List(Arc::new(vec.into_iter().map(|v| v.into()).collect()))
    }
}

impl From<i32> for LoroValue {
    fn from(v: i32) -> Self {
        LoroValue::I32(v)
    }
}

impl From<u8> for LoroValue {
    fn from(v: u8) -> Self {
        LoroValue::I32(v as i32)
    }
}

impl From<u16> for LoroValue {
    fn from(v: u16) -> Self {
        LoroValue::I32(v as i32)
    }
}

impl From<i16> for LoroValue {
    fn from(v: i16) -> Self {
        LoroValue::I32(v as i32)
    }
}

impl From<f64> for LoroValue {
    fn from(v: f64) -> Self {
        LoroValue::Double(v)
    }
}

impl From<bool> for LoroValue {
    fn from(v: bool) -> Self {
        LoroValue::Bool(v)
    }
}

impl From<&str> for LoroValue {
    fn from(v: &str) -> Self {
        LoroValue::String(Arc::new(v.to_string()))
    }
}

impl From<String> for LoroValue {
    fn from(v: String) -> Self {
        LoroValue::String(v.into())
    }
}

impl From<ContainerID> for LoroValue {
    fn from(v: ContainerID) -> Self {
        LoroValue::Container(v)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TypeHint {
    Map,
    Text,
    List,
}

impl LoroValue {
    fn get_mut(&mut self, path: &Path, last_hint: TypeHint) -> &mut LoroValue {
        let mut hints = Vec::with_capacity(path.len());
        for item in path.iter().skip(1) {
            match item {
                Index::Key(_) => hints.push(TypeHint::Map),
                Index::Seq(_) => hints.push(TypeHint::List),
            }
        }

        hints.push(last_hint);
        let mut value = self;
        for (item, hint) in path.iter().zip(hints.iter()) {
            match item {
                Index::Key(key) => {
                    let m = value.as_map_mut().unwrap();
                    let map = Arc::make_mut(m);
                    value = map.entry(key.to_string()).or_insert_with(|| match hint {
                        TypeHint::Map => LoroValue::Map(Default::default()),
                        TypeHint::Text => LoroValue::String(Arc::new(String::new())),
                        TypeHint::List => LoroValue::List(Default::default()),
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

    pub fn apply_diff(&mut self, diff: &[Diff]) {
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
                    let diff = item.as_map().unwrap();
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
            }
            _ => unreachable!(),
        }
    }

    pub fn apply(&mut self, path: &Path, diff: &[Diff]) {
        if diff.is_empty() {
            return;
        }

        let hint = match diff[0] {
            Diff::List(_) => TypeHint::List,
            Diff::Text(_) => TypeHint::Text,
            Diff::Map(_) => TypeHint::Map,
            Diff::NewMap(_) => TypeHint::Map,
            Diff::SeqRaw(_) => TypeHint::Text,
        };
        self.get_mut(path, hint).apply_diff(diff);
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

    use fxhash::FxHashMap;
    use js_sys::{Array, Object};
    use wasm_bindgen::{JsCast, JsValue, __rt::IntoJsResult};

    use crate::{
        container::ContainerID,
        delta::{Delta, DeltaItem, MapDiff},
        event::{Diff, Index, Utf16Meta},
        LoroError, LoroValue,
    };

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
        }
    }

    impl From<LoroValue> for JsValue {
        fn from(value: LoroValue) -> Self {
            convert(value)
        }
    }

    impl From<JsValue> for LoroValue {
        fn from(js_value: JsValue) -> Self {
            if js_value.is_null() {
                LoroValue::Null
            } else if js_value.as_bool().is_some() {
                LoroValue::Bool(js_value.as_bool().unwrap())
            } else if js_value.as_f64().is_some() {
                let num = js_value.as_f64().unwrap();
                if num.fract() == 0.0 {
                    LoroValue::I32(num as i32)
                } else {
                    LoroValue::Double(num)
                }
            } else if js_value.is_string() {
                LoroValue::String(Arc::new(js_value.as_string().unwrap()))
            } else if js_value.has_type::<Array>() {
                let array = js_value.unchecked_into::<Array>();
                let mut list = Vec::new();
                for i in 0..array.length() {
                    list.push(LoroValue::from(array.get(i)));
                }

                LoroValue::List(Arc::new(list))
            } else if js_value.is_object() {
                let object = js_value.unchecked_into::<Object>();
                let mut map = FxHashMap::default();
                for key in js_sys::Reflect::own_keys(&object).unwrap().iter() {
                    let key = key.as_string().unwrap();
                    map.insert(
                        key.clone(),
                        LoroValue::from(js_sys::Reflect::get(&object, &key.into()).unwrap()),
                    );
                }

                map.into()
            } else {
                unreachable!()
            }
        }
    }

    impl From<ContainerID> for JsValue {
        fn from(id: ContainerID) -> Self {
            JsValue::from_str(id.to_string().as_str())
        }
    }

    impl TryFrom<JsValue> for ContainerID {
        type Error = LoroError;

        fn try_from(value: JsValue) -> Result<Self, Self::Error> {
            if !value.is_string() {
                return Err(LoroError::DecodeError(
                    "Given ContainerId is not string".into(),
                ));
            }

            let s = value.as_string().unwrap();
            ContainerID::try_from(s.as_str()).map_err(|_| {
                LoroError::DecodeError(
                    format!("Given ContainerId is not a valid ContainerID: {}", s).into(),
                )
            })
        }
    }

    impl From<Index> for JsValue {
        fn from(value: Index) -> Self {
            match value {
                Index::Key(key) => JsValue::from_str(&key),
                Index::Seq(num) => JsValue::from_f64(num as f64),
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

                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("diff"),
                        &serde_wasm_bindgen::to_value(&map).unwrap(),
                    )
                    .unwrap();
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
            };

            // convert object to js value
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

    impl From<Delta<String, Utf16Meta>> for JsValue {
        fn from(value: Delta<String, Utf16Meta>) -> Self {
            let arr = Array::new_with_length(value.len() as u32);
            for (i, v) in value.iter().enumerate() {
                arr.set(i as u32, JsValue::from(v.clone()));
            }

            arr.into_js_result().unwrap()
        }
    }

    impl From<DeltaItem<String, Utf16Meta>> for JsValue {
        fn from(value: DeltaItem<String, Utf16Meta>) -> Self {
            let obj = Object::new();
            match value {
                DeltaItem::Retain { len: _len, meta } => {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("retain"),
                    )
                    .unwrap();
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("len"),
                        &JsValue::from_f64(meta.utf16_len.unwrap() as f64),
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
                DeltaItem::Delete { len: _len, meta } => {
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("type"),
                        &JsValue::from_str("delete"),
                    )
                    .unwrap();
                    js_sys::Reflect::set(
                        &obj,
                        &JsValue::from_str("len"),
                        &JsValue::from_f64(meta.utf16_len.unwrap() as f64),
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
pub(crate) mod proptest {
    use proptest::prelude::*;
    use proptest::prop_oneof;

    use super::LoroValue;

    pub fn gen_insert_value() -> impl Strategy<Value = LoroValue> {
        prop_oneof![
            Just(LoroValue::Null),
            any::<f64>().prop_map(LoroValue::Double),
            any::<i32>().prop_map(LoroValue::I32),
            any::<bool>().prop_map(LoroValue::Bool),
            any::<String>().prop_map(|s| LoroValue::String(s.into())),
        ]
    }
}

impl Serialize for LoroValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if serializer.is_human_readable() {
            // json type
            match self {
                LoroValue::Null => serializer.serialize_unit(),
                LoroValue::Bool(b) => serializer.serialize_bool(*b),
                LoroValue::Double(d) => serializer.serialize_f64(*d),
                LoroValue::I32(i) => serializer.serialize_i32(*i),
                LoroValue::String(s) => serializer.serialize_str(s),
                LoroValue::List(l) => serializer.collect_seq(l.iter()),
                LoroValue::Map(m) => serializer.collect_map(m.iter()),
                LoroValue::Container(id) => {
                    let mut state = serializer.serialize_struct("Unresolved", 1)?;
                    state.serialize_field("Unresolved", id)?;
                    state.end()
                }
            }
        } else {
            // binary type
            match self {
                LoroValue::Null => serializer.serialize_unit_variant("LoroValue", 0, "Null"),
                LoroValue::Bool(b) => {
                    serializer.serialize_newtype_variant("LoroValue", 1, "Bool", b)
                }
                LoroValue::Double(d) => {
                    serializer.serialize_newtype_variant("LoroValue", 2, "Double", d)
                }
                LoroValue::I32(i) => serializer.serialize_newtype_variant("LoroValue", 3, "I32", i),
                LoroValue::String(s) => {
                    serializer.serialize_newtype_variant("LoroValue", 4, "String", &**s)
                }
                LoroValue::List(l) => {
                    serializer.serialize_newtype_variant("LoroValue", 5, "List", &**l)
                }
                LoroValue::Map(m) => {
                    serializer.serialize_newtype_variant("LoroValue", 6, "Map", &**m)
                }
                LoroValue::Container(id) => {
                    serializer.serialize_newtype_variant("LoroValue", 7, "Unresolved", id)
                }
            }
        }
    }
}

impl<'de> Deserialize<'de> for LoroValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            deserializer.deserialize_any(LoroValueVisitor)
        } else {
            deserializer.deserialize_enum(
                "LoroValue",
                &[
                    "Null",
                    "Bool",
                    "Double",
                    "I32",
                    "String",
                    "List",
                    "Map",
                    "Unresolved",
                ],
                LoroValueEnumVisitor,
            )
        }
    }
}

struct LoroValueVisitor;

impl<'de> serde::de::Visitor<'de> for LoroValueVisitor {
    type Value = LoroValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a LoroValue")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(LoroValue::Null)
    }

    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(LoroValue::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(LoroValue::I32(v as i32))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(LoroValue::I32(v as i32))
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(LoroValue::Double(v))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(LoroValue::String(Arc::new(v.to_owned())))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(LoroValue::String(v.into()))
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut list = Vec::new();
        while let Some(value) = seq.next_element()? {
            list.push(value);
        }
        Ok(LoroValue::List(list.into()))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut ans: FxHashMap<String, _> = FxHashMap::default();
        let mut last_key = None;
        while let Some((key, value)) = map.next_entry::<String, _>()? {
            last_key.get_or_insert_with(|| key.clone());
            ans.insert(key, value);
        }

        Ok(LoroValue::Map(ans.into()))
    }
}

#[derive(Deserialize)]
enum LoroValueFields {
    Null,
    Bool,
    Double,
    I32,
    String,
    List,
    Map,
    Unresolved,
}

struct LoroValueEnumVisitor;
impl<'de> serde::de::Visitor<'de> for LoroValueEnumVisitor {
    type Value = LoroValue;
    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a loro value")
    }

    fn visit_enum<A>(self, data: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::EnumAccess<'de>,
    {
        match data.variant()? {
            (LoroValueFields::Null, v) => {
                v.unit_variant()?;
                Ok(LoroValue::Null)
            }
            (LoroValueFields::Bool, v) => v.newtype_variant().map(LoroValue::Bool),
            (LoroValueFields::Double, v) => v.newtype_variant().map(LoroValue::Double),
            (LoroValueFields::I32, v) => v.newtype_variant().map(LoroValue::I32),
            (LoroValueFields::String, v) => {
                v.newtype_variant().map(|x| LoroValue::String(Arc::new(x)))
            }
            (LoroValueFields::List, v) => v.newtype_variant().map(|x| LoroValue::List(Arc::new(x))),
            (LoroValueFields::Map, v) => v.newtype_variant().map(|x| LoroValue::Map(Arc::new(x))),
            (LoroValueFields::Unresolved, v) => {
                v.newtype_variant().map(|x| LoroValue::Container(x))
            }
        }
    }
}

#[cfg(test)]
#[cfg(feature = "json")]
mod json_test {
    use crate::{fx_map, LoroValue};

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
