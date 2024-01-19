use std::{collections::HashMap, hash::Hash, ops::Index, sync::Arc};

use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use serde::{de::VariantAccess, ser::SerializeStruct, Deserialize, Serialize};

use crate::ContainerID;

/// [LoroValue] is used to represents the state of CRDT at a given version.
///
/// This struct is cheap to clone, the time complexity is O(1).
#[derive(Debug, PartialEq, Clone, EnumAsInner, Default)]
pub enum LoroValue {
    #[default]
    Null,
    Bool(bool),
    Double(f64),
    I32(i32),
    // i64?
    Binary(Arc<Vec<u8>>),
    String(Arc<String>),
    List(Arc<Vec<LoroValue>>),
    // PERF We can use InternalString as key
    Map(Arc<FxHashMap<String, LoroValue>>),
    Container(ContainerID),
}

const MAX_DEPTH: usize = 128;
impl<'a> arbitrary::Arbitrary<'a> for LoroValue {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let value = match u.int_in_range(0..=7).unwrap() {
            0 => LoroValue::Null,
            1 => LoroValue::Bool(u.arbitrary()?),
            2 => LoroValue::Double(u.arbitrary()?),
            3 => LoroValue::I32(u.arbitrary()?),
            4 => LoroValue::Binary(Arc::new(u.arbitrary()?)),
            5 => LoroValue::String(Arc::new(u.arbitrary()?)),
            6 => LoroValue::List(Arc::new(u.arbitrary()?)),
            7 => LoroValue::Map(Arc::new(u.arbitrary()?)),
            _ => unreachable!(),
        };

        if value.get_depth() > MAX_DEPTH {
            Err(arbitrary::Error::IncorrectFormat)
        } else {
            Ok(value)
        }
    }
}

impl LoroValue {
    pub fn get_by_key(&self, key: &str) -> Option<&LoroValue> {
        match self {
            LoroValue::Map(map) => map.get(key),
            _ => None,
        }
    }

    pub fn get_by_index(&self, index: usize) -> Option<&LoroValue> {
        match self {
            LoroValue::List(list) => list.get(index),
            _ => None,
        }
    }

    pub fn is_false(&self) -> bool {
        match self {
            LoroValue::Bool(b) => !*b,
            _ => false,
        }
    }

    pub fn get_depth(&self) -> usize {
        let mut max_depth = 0;
        let mut value_depth_pairs = vec![(self, 0)];
        while let Some((value, depth)) = value_depth_pairs.pop() {
            match value {
                LoroValue::List(arr) => {
                    for v in arr.iter() {
                        value_depth_pairs.push((v, depth + 1));
                    }
                    max_depth = max_depth.max(depth + 1);
                }
                LoroValue::Map(map) => {
                    for (_, v) in map.iter() {
                        value_depth_pairs.push((v, depth + 1));
                    }

                    max_depth = max_depth.max(depth + 1);
                }
                _ => {}
            }
        }

        max_depth
    }

    // TODO: add checks for too deep value, and return err if users
    // try to insert such value into a container
    pub fn is_too_deep(&self) -> bool {
        self.get_depth() > MAX_DEPTH
    }
}

impl Index<&str> for LoroValue {
    type Output = LoroValue;

    fn index(&self, index: &str) -> &Self::Output {
        match self {
            LoroValue::Map(map) => map.get(index).unwrap_or(&LoroValue::Null),
            _ => &LoroValue::Null,
        }
    }
}

impl Index<usize> for LoroValue {
    type Output = LoroValue;

    fn index(&self, index: usize) -> &Self::Output {
        match self {
            LoroValue::List(list) => list.get(index).unwrap_or(&LoroValue::Null),
            _ => &LoroValue::Null,
        }
    }
}

impl TryFrom<LoroValue> for bool {
    type Error = &'static str;

    fn try_from(value: LoroValue) -> Result<Self, Self::Error> {
        match value {
            LoroValue::Bool(v) => Ok(v),
            _ => Err("not a bool"),
        }
    }
}

impl TryFrom<LoroValue> for f64 {
    type Error = &'static str;

    fn try_from(value: LoroValue) -> Result<Self, Self::Error> {
        match value {
            LoroValue::Double(v) => Ok(v),
            _ => Err("not a double"),
        }
    }
}

impl TryFrom<LoroValue> for i32 {
    type Error = &'static str;

    fn try_from(value: LoroValue) -> Result<Self, Self::Error> {
        match value {
            LoroValue::I32(v) => Ok(v),
            _ => Err("not a i32"),
        }
    }
}

impl TryFrom<LoroValue> for Arc<Vec<u8>> {
    type Error = &'static str;

    fn try_from(value: LoroValue) -> Result<Self, Self::Error> {
        match value {
            LoroValue::Binary(v) => Ok(v),
            _ => Err("not a binary"),
        }
    }
}

impl TryFrom<LoroValue> for Arc<String> {
    type Error = &'static str;

    fn try_from(value: LoroValue) -> Result<Self, Self::Error> {
        match value {
            LoroValue::String(v) => Ok(v),
            _ => Err("not a string"),
        }
    }
}

impl TryFrom<LoroValue> for Arc<Vec<LoroValue>> {
    type Error = &'static str;

    fn try_from(value: LoroValue) -> Result<Self, Self::Error> {
        match value {
            LoroValue::List(v) => Ok(v),
            _ => Err("not a list"),
        }
    }
}

impl TryFrom<LoroValue> for Arc<FxHashMap<String, LoroValue>> {
    type Error = &'static str;

    fn try_from(value: LoroValue) -> Result<Self, Self::Error> {
        match value {
            LoroValue::Map(v) => Ok(v),
            _ => Err("not a map"),
        }
    }
}

impl TryFrom<LoroValue> for ContainerID {
    type Error = &'static str;

    fn try_from(value: LoroValue) -> Result<Self, Self::Error> {
        match value {
            LoroValue::Container(v) => Ok(v),
            _ => Err("not a container"),
        }
    }
}

impl Hash for LoroValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            LoroValue::Null => {}
            LoroValue::Bool(v) => {
                state.write_u8(*v as u8);
            }
            LoroValue::Double(v) => {
                state.write_u64(v.to_bits());
            }
            LoroValue::I32(v) => {
                state.write_i32(*v);
            }
            LoroValue::Binary(v) => {
                v.hash(state);
            }
            LoroValue::String(v) => {
                v.hash(state);
            }
            LoroValue::List(v) => {
                v.hash(state);
            }
            LoroValue::Map(v) => {
                state.write_usize(v.len());
                for (k, v) in v.iter() {
                    k.hash(state);
                    v.hash(state);
                }
            }
            LoroValue::Container(v) => {
                v.hash(state);
            }
        }
    }
}

impl Eq for LoroValue {}

impl<S: Into<String>, M> From<HashMap<S, LoroValue, M>> for LoroValue {
    fn from(map: HashMap<S, LoroValue, M>) -> Self {
        let mut new_map = FxHashMap::default();
        for (k, v) in map {
            new_map.insert(k.into(), v);
        }

        LoroValue::Map(Arc::new(new_map))
    }
}

impl From<Vec<u8>> for LoroValue {
    fn from(vec: Vec<u8>) -> Self {
        LoroValue::Binary(Arc::new(vec))
    }
}

impl From<&'_ [u8]> for LoroValue {
    fn from(vec: &[u8]) -> Self {
        LoroValue::Binary(Arc::new(vec.to_vec()))
    }
}

impl<const N: usize> From<&'_ [u8; N]> for LoroValue {
    fn from(vec: &[u8; N]) -> Self {
        LoroValue::Binary(Arc::new(vec.to_vec()))
    }
}

impl From<i32> for LoroValue {
    fn from(v: i32) -> Self {
        LoroValue::I32(v)
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

impl<T: Into<LoroValue>> From<Vec<T>> for LoroValue {
    fn from(value: Vec<T>) -> Self {
        let vec: Vec<LoroValue> = value.into_iter().map(|x| x.into()).collect();
        Self::List(Arc::new(vec))
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

impl<'a> From<&'a [LoroValue]> for LoroValue {
    fn from(v: &'a [LoroValue]) -> Self {
        LoroValue::List(Arc::new(v.to_vec()))
    }
}

impl From<ContainerID> for LoroValue {
    fn from(v: ContainerID) -> Self {
        LoroValue::Container(v)
    }
}

#[cfg(feature = "wasm")]
pub mod wasm {
    use std::sync::Arc;

    use fxhash::FxHashMap;
    use js_sys::{Array, Object, Uint8Array};
    use wasm_bindgen::{JsCast, JsValue, __rt::IntoJsResult};

    use crate::{ContainerID, LoroError, LoroValue};

    pub fn convert(value: LoroValue) -> JsValue {
        match value {
            LoroValue::Null => JsValue::NULL,
            LoroValue::Bool(b) => JsValue::from_bool(b),
            LoroValue::Double(f) => JsValue::from_f64(f),
            LoroValue::I32(i) => JsValue::from_f64(i as f64),
            LoroValue::String(s) => JsValue::from_str(&s),
            LoroValue::Binary(binary) => {
                let binary = Arc::try_unwrap(binary).unwrap_or_else(|m| (*m).clone());
                let arr = Uint8Array::new_with_length(binary.len() as u32);
                for (i, v) in binary.into_iter().enumerate() {
                    arr.set_index(i as u32, v);
                }
                arr.into_js_result().unwrap()
            }
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
                if num.fract() == 0.0 && num <= i32::MAX as f64 && num >= i32::MIN as f64 {
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
                LoroValue::Binary(b) => serializer.collect_seq(b.iter()),
                LoroValue::List(l) => serializer.collect_seq(l.iter()),
                LoroValue::Map(m) => serializer.collect_map(m.iter()),
                LoroValue::Container(id) => {
                    let mut state = serializer.serialize_struct("Container", 1)?;
                    state.serialize_field("Container", id)?;
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
                    serializer.serialize_newtype_variant("LoroValue", 7, "Container", id)
                }
                LoroValue::Binary(b) => {
                    serializer.serialize_newtype_variant("LoroValue", 8, "Binary", &**b)
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
                    "Container",
                    "Binary",
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

    fn visit_borrowed_bytes<E>(self, v: &'de [u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let binary = Vec::from_iter(v.iter().copied());
        Ok(LoroValue::Binary(binary.into()))
    }

    fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let binary = Vec::from_iter(v.iter().copied());
        Ok(LoroValue::Binary(binary.into()))
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
    Container,
    Binary,
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
            (LoroValueFields::Container, v) => v.newtype_variant().map(LoroValue::Container),
            (LoroValueFields::Binary, v) => {
                v.newtype_variant().map(|x| LoroValue::Binary(Arc::new(x)))
            }
        }
    }
}

pub fn to_value<T: Into<LoroValue>>(value: T) -> LoroValue {
    value.into()
}
