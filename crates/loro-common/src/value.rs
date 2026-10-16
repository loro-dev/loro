use std::{
    collections::HashMap,
    hash::Hash,
    ops::Index,
    sync::{Arc, OnceLock},
};

use arbitrary::Arbitrary;
use enum_as_inner::EnumAsInner;
use rustc_hash::FxHashMap;
use serde::{
    de::{DeserializeSeed, VariantAccess},
    Deserialize, Serialize,
};

use crate::ContainerID;

/// [LoroValue] is used to represents the state of CRDT at a given version.
///
/// This struct is cheap to clone, the time complexity is O(1).
#[derive(Debug, Clone, EnumAsInner, Default)]
pub enum LoroValue {
    #[default]
    Null,
    Bool(bool),
    Double(f64),
    I64(i64),
    // i64?
    Binary(LoroBinaryValue),
    String(LoroStringValue),
    List(LoroListValue),
    // PERF We can use InternalString as key
    Map(LoroMapValue),
    Container(ContainerID),
}

impl PartialEq for LoroValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (LoroValue::Null, LoroValue::Null) => true,
            (LoroValue::Bool(a), LoroValue::Bool(b)) => a == b,
            (LoroValue::Double(a), LoroValue::Double(b)) => a == b || (a.is_nan() && b.is_nan()),
            (LoroValue::I64(a), LoroValue::I64(b)) => a == b,
            (LoroValue::Binary(a), LoroValue::Binary(b)) => a == b,
            (LoroValue::String(a), LoroValue::String(b)) => a == b,
            (LoroValue::List(a), LoroValue::List(b)) => a == b,
            (LoroValue::Map(a), LoroValue::Map(b)) => a == b,
            (LoroValue::Container(a), LoroValue::Container(b)) => a == b,
            _ => false,
        }
    }
}

#[derive(Default, Debug, PartialEq, Clone, Arbitrary)]
pub struct LoroBinaryValue(Arc<Vec<u8>>);
#[derive(Default, Debug, PartialEq, Clone, Arbitrary)]
pub struct LoroStringValue(Arc<String>);
#[derive(Default, Debug, PartialEq, Clone, Arbitrary)]
pub struct LoroListValue(Arc<Vec<LoroValue>>);
#[derive(Default, Debug, PartialEq, Clone, Arbitrary)]
pub struct LoroMapValue(Arc<FxHashMap<String, LoroValue>>);

impl From<Vec<u8>> for LoroBinaryValue {
    fn from(value: Vec<u8>) -> Self {
        LoroBinaryValue(Arc::new(value))
    }
}

impl From<String> for LoroStringValue {
    fn from(value: String) -> Self {
        LoroStringValue(Arc::new(value))
    }
}

impl From<&str> for LoroStringValue {
    fn from(value: &str) -> Self {
        LoroStringValue(Arc::new(value.to_string()))
    }
}

impl From<Vec<LoroValue>> for LoroListValue {
    fn from(value: Vec<LoroValue>) -> Self {
        LoroListValue(Arc::new(value))
    }
}

impl From<FxHashMap<String, LoroValue>> for LoroMapValue {
    fn from(value: FxHashMap<String, LoroValue>) -> Self {
        LoroMapValue(Arc::new(value))
    }
}

impl From<HashMap<String, LoroValue>> for LoroMapValue {
    fn from(value: HashMap<String, LoroValue>) -> Self {
        LoroMapValue(Arc::new(FxHashMap::from_iter(value)))
    }
}

impl From<Vec<(String, LoroValue)>> for LoroMapValue {
    fn from(value: Vec<(String, LoroValue)>) -> Self {
        LoroMapValue(Arc::new(FxHashMap::from_iter(value)))
    }
}

impl std::ops::Deref for LoroBinaryValue {
    type Target = Vec<u8>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::Deref for LoroStringValue {
    type Target = String;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::Deref for LoroListValue {
    type Target = Vec<LoroValue>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::Deref for LoroMapValue {
    type Target = FxHashMap<String, LoroValue>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl LoroBinaryValue {
    pub fn make_mut(&mut self) -> &mut Vec<u8> {
        Arc::make_mut(&mut self.0)
    }
}

impl LoroStringValue {
    pub fn make_mut(&mut self) -> &mut String {
        Arc::make_mut(&mut self.0)
    }
}

impl LoroListValue {
    pub fn make_mut(&mut self) -> &mut Vec<LoroValue> {
        Arc::make_mut(&mut self.0)
    }

    /// The shared list, without copying it.
    pub fn into_arc(mut self) -> Arc<Vec<LoroValue>> {
        std::mem::take(&mut self.0)
    }
}

impl LoroMapValue {
    pub fn make_mut(&mut self) -> &mut FxHashMap<String, LoroValue> {
        Arc::make_mut(&mut self.0)
    }
}

impl LoroBinaryValue {
    pub fn unwrap(self) -> Vec<u8> {
        match Arc::try_unwrap(self.0) {
            Ok(v) => v,
            Err(arc) => (*arc).clone(),
        }
    }
}

impl LoroStringValue {
    pub fn unwrap(self) -> String {
        match Arc::try_unwrap(self.0) {
            Ok(v) => v,
            Err(arc) => (*arc).clone(),
        }
    }
}

impl LoroListValue {
    pub fn unwrap(mut self) -> Vec<LoroValue> {
        match Arc::try_unwrap(std::mem::take(&mut self.0)) {
            Ok(v) => v,
            Err(arc) => (*arc).clone(),
        }
    }
}

impl LoroMapValue {
    pub fn unwrap(mut self) -> FxHashMap<String, LoroValue> {
        match Arc::try_unwrap(std::mem::take(&mut self.0)) {
            Ok(v) => v,
            Err(arc) => (*arc).clone(),
        }
    }
}

// Detach children before dropping them so the last Arc never recursively frees
// an arbitrarily deep value. Arc::into_inner also handles concurrent final drops:
// unlike try_unwrap(...).ok(), exactly one caller takes the shared allocation.
static EMPTY_LIST: OnceLock<Arc<Vec<LoroValue>>> = OnceLock::new();
static EMPTY_MAP: OnceLock<Arc<FxHashMap<String, LoroValue>>> = OnceLock::new();

fn take_children<T: Default>(arc: &mut Arc<T>, empty: &OnceLock<Arc<T>>) -> Option<T> {
    if let Some(children) = Arc::get_mut(arc) {
        Some(std::mem::take(children))
    } else {
        // Shared value drops are common; reuse an empty Arc instead of allocating.
        Arc::into_inner(std::mem::replace(
            arc,
            empty.get_or_init(|| Arc::new(T::default())).clone(),
        ))
    }
}

fn drop_values(mut pending: Vec<LoroValue>) {
    while let Some(mut value) = pending.pop() {
        match &mut value {
            LoroValue::List(list) if !list.is_empty() => {
                if let Some(children) = take_children(&mut list.0, &EMPTY_LIST) {
                    pending.extend(children);
                }
            }
            LoroValue::Map(map) if !map.is_empty() => {
                if let Some(children) = take_children(&mut map.0, &EMPTY_MAP) {
                    pending.extend(children.into_values());
                }
            }
            _ => {}
        }
    }
}

impl Drop for LoroListValue {
    fn drop(&mut self) {
        if !self.is_empty() {
            if let Some(children) = take_children(&mut self.0, &EMPTY_LIST) {
                drop_values(children);
            }
        }
    }
}

impl Drop for LoroMapValue {
    fn drop(&mut self) {
        if !self.is_empty() {
            if let Some(children) = take_children(&mut self.0, &EMPTY_MAP) {
                drop_values(children.into_values().collect());
            }
        }
    }
}

impl FromIterator<LoroValue> for LoroListValue {
    fn from_iter<T: IntoIterator<Item = LoroValue>>(iter: T) -> Self {
        LoroListValue(Arc::new(iter.into_iter().collect()))
    }
}

impl FromIterator<(String, LoroValue)> for LoroMapValue {
    fn from_iter<T: IntoIterator<Item = (String, LoroValue)>>(iter: T) -> Self {
        let map: FxHashMap<_, _> = iter.into_iter().collect();
        LoroMapValue(Arc::new(map))
    }
}

impl AsRef<str> for LoroStringValue {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<[LoroValue]> for LoroListValue {
    fn as_ref(&self) -> &[LoroValue] {
        &self.0
    }
}

impl AsRef<FxHashMap<String, LoroValue>> for LoroMapValue {
    fn as_ref(&self) -> &FxHashMap<String, LoroValue> {
        &self.0
    }
}

const MAX_DEPTH: usize = 128;
impl<'a> arbitrary::Arbitrary<'a> for LoroValue {
    fn arbitrary(u: &mut arbitrary::Unstructured<'a>) -> arbitrary::Result<Self> {
        let value = match u.int_in_range(0..=7).unwrap() {
            0 => LoroValue::Null,
            1 => LoroValue::Bool(u.arbitrary()?),
            2 => LoroValue::Double(u.arbitrary()?),
            3 => LoroValue::I64(u.arbitrary()?),
            4 => LoroValue::Binary(LoroBinaryValue::arbitrary(u)?),
            5 => LoroValue::String(LoroStringValue::arbitrary(u)?),
            6 => LoroValue::List(LoroListValue::arbitrary(u)?),
            7 => LoroValue::Map(LoroMapValue::arbitrary(u)?),
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

    pub fn get_by_index(&self, index: isize) -> Option<&LoroValue> {
        match self {
            LoroValue::List(list) => {
                if index < 0 {
                    let offset = usize::try_from(index.checked_neg()?).ok()?;
                    list.get(list.len().checked_sub(offset)?)
                } else {
                    list.get(usize::try_from(index).ok()?)
                }
            }
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

    /// Visit the all list items or map's values
    pub fn visit_children(&self, f: &mut dyn FnMut(&LoroValue)) {
        match self {
            LoroValue::List(list) => {
                for v in list.iter() {
                    f(v);
                }
            }
            LoroValue::Map(m) => {
                for (_k, v) in m.iter() {
                    f(v)
                }
            }
            _ => {}
        }
    }

    pub fn is_empty_collection(&self) -> bool {
        match self {
            LoroValue::Null => false,
            LoroValue::Bool(_) => false,
            LoroValue::Double(_) => false,
            LoroValue::I64(_) => false,
            LoroValue::Binary(loro_binary_value) => loro_binary_value.is_empty(),
            LoroValue::String(loro_string_value) => loro_string_value.is_empty(),
            LoroValue::List(loro_list_value) => loro_list_value.is_empty(),
            LoroValue::Map(loro_map_value) => loro_map_value.is_empty(),
            LoroValue::Container(_) => false,
        }
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
            LoroValue::I64(v) => i32::try_from(v).map_err(|_| "i64 out of i32 range"),
            _ => Err("not a i32"),
        }
    }
}

impl TryFrom<LoroValue> for Arc<Vec<u8>> {
    type Error = &'static str;

    fn try_from(value: LoroValue) -> Result<Self, Self::Error> {
        match value {
            LoroValue::Binary(v) => Ok(Arc::clone(&v.0)),
            _ => Err("not a binary"),
        }
    }
}

impl TryFrom<LoroValue> for Arc<String> {
    type Error = &'static str;

    fn try_from(value: LoroValue) -> Result<Self, Self::Error> {
        match value {
            LoroValue::String(v) => Ok(Arc::clone(&v.0)),
            _ => Err("not a string"),
        }
    }
}

impl TryFrom<LoroValue> for Arc<Vec<LoroValue>> {
    type Error = &'static str;

    fn try_from(value: LoroValue) -> Result<Self, Self::Error> {
        match value {
            LoroValue::List(v) => Ok(Arc::clone(&v.0)),
            _ => Err("not a list"),
        }
    }
}

impl TryFrom<LoroValue> for Arc<FxHashMap<String, LoroValue>> {
    type Error = &'static str;

    fn try_from(value: LoroValue) -> Result<Self, Self::Error> {
        match value {
            LoroValue::Map(v) => Ok(Arc::clone(&v.0)),
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
                let normalized = if v.is_nan() {
                    f64::NAN
                } else if *v == 0.0 {
                    0.0
                } else {
                    *v
                };
                state.write_u64(normalized.to_bits());
            }
            LoroValue::I64(v) => {
                state.write_i64(*v);
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
                let mut entries: Vec<_> = v.iter().collect();
                entries.sort_unstable_by_key(|(k, _)| *k);
                for (k, v) in entries {
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

        LoroValue::Map(new_map.into())
    }
}

impl From<Vec<u8>> for LoroValue {
    fn from(vec: Vec<u8>) -> Self {
        LoroValue::Binary(vec.into())
    }
}

impl From<&'_ [u8]> for LoroValue {
    fn from(vec: &[u8]) -> Self {
        LoroValue::Binary(vec.to_vec().into())
    }
}

impl<const N: usize> From<&'_ [u8; N]> for LoroValue {
    fn from(vec: &[u8; N]) -> Self {
        LoroValue::Binary(vec.to_vec().into())
    }
}

impl From<i32> for LoroValue {
    fn from(v: i32) -> Self {
        LoroValue::I64(v as i64)
    }
}

impl From<u32> for LoroValue {
    fn from(v: u32) -> Self {
        LoroValue::I64(v as i64)
    }
}

impl From<i64> for LoroValue {
    fn from(v: i64) -> Self {
        LoroValue::I64(v)
    }
}

impl From<u16> for LoroValue {
    fn from(v: u16) -> Self {
        LoroValue::I64(v as i64)
    }
}

impl From<i16> for LoroValue {
    fn from(v: i16) -> Self {
        LoroValue::I64(v as i64)
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
        LoroValue::List(vec.into())
    }
}

impl From<&str> for LoroValue {
    fn from(v: &str) -> Self {
        LoroValue::String(v.to_string().into())
    }
}

impl From<String> for LoroValue {
    fn from(v: String) -> Self {
        LoroValue::String(v.into())
    }
}

impl<'a> From<&'a [LoroValue]> for LoroValue {
    fn from(v: &'a [LoroValue]) -> Self {
        LoroValue::List(v.to_vec().into())
    }
}

impl From<ContainerID> for LoroValue {
    fn from(v: ContainerID) -> Self {
        LoroValue::Container(v)
    }
}

#[cfg(feature = "wasm")]
pub mod wasm {
    use js_sys::{Array, Object, Uint8Array};
    use rustc_hash::FxHashMap;
    use wasm_bindgen::{JsCast, JsValue};

    use crate::{ContainerID, LoroError, LoroValue};

    pub fn convert(value: LoroValue) -> JsValue {
        try_convert(value).unwrap()
    }

    /// Convert nested values using heap storage rather than the WASM call stack.
    pub fn try_convert(value: LoroValue) -> Result<JsValue, JsValue> {
        let mut pending = vec![(&value, None::<(JsValue, JsValue)>)];
        let mut result = JsValue::UNDEFINED;
        while let Some((value, parent)) = pending.pop() {
            let output: JsValue = match value {
                LoroValue::Null => JsValue::NULL,
                LoroValue::Bool(b) => JsValue::from_bool(*b),
                LoroValue::Double(f) => JsValue::from_f64(*f),
                LoroValue::I64(i) => JsValue::from_f64(*i as f64),
                LoroValue::String(s) => JsValue::from_str(s),
                LoroValue::Binary(binary) => Uint8Array::from(binary.as_slice()).into(),
                LoroValue::Container(id) => JsValue::from(id),
                LoroValue::List(list) => {
                    let array: JsValue = Array::new_with_length(list.len() as u32).into();
                    for (i, child) in list.iter().enumerate().rev() {
                        pending.push((child, Some((array.clone(), JsValue::from_f64(i as f64)))));
                    }
                    array
                }
                LoroValue::Map(map) => {
                    let object: JsValue = Object::new().into();
                    let start = pending.len();
                    for (key, child) in map.iter() {
                        pending.push((child, Some((object.clone(), JsValue::from_str(key)))));
                    }
                    pending[start..].reverse();
                    object
                }
            };
            if let Some((parent, key)) = parent {
                js_sys::Reflect::set(&parent, &key, &output)?;
            } else {
                result = output;
            }
        }
        Ok(result)
    }

    impl From<LoroValue> for JsValue {
        fn from(value: LoroValue) -> Self {
            convert(value)
        }
    }

    impl From<JsValue> for LoroValue {
        fn from(js_value: JsValue) -> Self {
            if js_value.is_null() || js_value.is_undefined() {
                LoroValue::Null
            } else if js_value.as_bool().is_some() {
                LoroValue::Bool(js_value.as_bool().unwrap())
            } else if js_value.as_f64().is_some() {
                let num = js_value.as_f64().unwrap();
                if num.fract() == 0.0 && num <= i64::MAX as f64 && num >= i64::MIN as f64 {
                    LoroValue::I64(num as i64)
                } else {
                    LoroValue::Double(num)
                }
            } else if js_value.is_string() {
                LoroValue::String(js_value.as_string().unwrap().into())
            } else if js_value.has_type::<Array>() {
                let array = js_value.unchecked_into::<Array>();
                let mut list = Vec::new();
                for i in 0..array.length() {
                    list.push(LoroValue::from(array.get(i)));
                }

                LoroValue::List(list.into())
            } else if js_value.is_instance_of::<Uint8Array>() {
                let array = js_value.unchecked_into::<Uint8Array>();
                let mut binary = Vec::new();
                for i in 0..array.length() {
                    binary.push(array.get_index(i));
                }

                LoroValue::Binary(binary.into())
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

                LoroValue::Map(map.into())
            } else {
                panic!("Fail to convert JsValue {js_value:?} to LoroValue ")
            }
        }
    }

    impl From<&ContainerID> for JsValue {
        fn from(id: &ContainerID) -> Self {
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
                    format!("Given ContainerId is not a valid ContainerID: {s}").into(),
                )
            })
        }
    }
}

const LORO_CONTAINER_ID_PREFIX: &str = "🦜:";

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
                LoroValue::I64(i) => serializer.serialize_i64(*i),
                LoroValue::String(s) => serializer.serialize_str(s),
                LoroValue::Binary(b) => serializer.collect_seq(b.iter()),
                LoroValue::List(l) => serializer.collect_seq(l.iter()),
                LoroValue::Map(m) => serializer.collect_map(m.iter()),
                LoroValue::Container(id) => {
                    serializer.serialize_str(&format!("{LORO_CONTAINER_ID_PREFIX}{id}"))
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
                LoroValue::I64(i) => serializer.serialize_newtype_variant("LoroValue", 3, "I32", i),
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
        LoroValueSeed {
            remaining_depth: MAX_DEPTH,
        }
        .deserialize(deserializer)
    }
}

#[derive(Clone, Copy)]
struct LoroValueSeed {
    remaining_depth: usize,
}

impl LoroValueSeed {
    fn nested<E: serde::de::Error>(self) -> Result<Self, E> {
        Ok(Self {
            remaining_depth: self
                .remaining_depth
                .checked_sub(1)
                .ok_or_else(|| E::custom("LoroValue nesting depth exceeds 128"))?,
        })
    }
}

impl<'de> DeserializeSeed<'de> for LoroValueSeed {
    type Value = LoroValue;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if deserializer.is_human_readable() {
            deserializer.deserialize_any(LoroValueVisitor(self))
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
                LoroValueEnumVisitor(self),
            )
        }
    }
}

struct LoroValueVisitor(LoroValueSeed);

// Binary enum payloads contain a bare sequence/map, rather than another enum.
struct LoroCollectionSeed {
    seed: LoroValueSeed,
    is_map: bool,
}

impl<'de> DeserializeSeed<'de> for LoroCollectionSeed {
    type Value = LoroValue;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        if self.is_map {
            deserializer.deserialize_map(LoroValueVisitor(self.seed))
        } else {
            deserializer.deserialize_seq(LoroValueVisitor(self.seed))
        }
    }
}

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
        Ok(LoroValue::I64(v))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(i64::try_from(v)
            .map(LoroValue::I64)
            .unwrap_or_else(|_| LoroValue::Double(v as f64)))
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
        if let Some(id) = v.strip_prefix(LORO_CONTAINER_ID_PREFIX) {
            return Ok(LoroValue::Container(
                ContainerID::try_from(id)
                    .map_err(|_| serde::de::Error::custom("Invalid container id"))?,
            ));
        }
        Ok(LoroValue::String(v.to_owned().into()))
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        if let Some(id) = v.strip_prefix(LORO_CONTAINER_ID_PREFIX) {
            return Ok(LoroValue::Container(
                ContainerID::try_from(id)
                    .map_err(|_| serde::de::Error::custom("Invalid container id"))?,
            ));
        }

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
        let seed = self.0.nested()?;
        let mut list = Vec::new();
        while let Some(value) = seq.next_element_seed(seed)? {
            list.push(value);
        }
        Ok(LoroValue::List(list.into()))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let seed = self.0.nested()?;
        let mut ans: FxHashMap<String, _> = FxHashMap::default();
        while let Some(key) = map.next_key::<String>()? {
            ans.insert(key, map.next_value_seed(seed)?);
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

struct LoroValueEnumVisitor(LoroValueSeed);
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
            (LoroValueFields::I32, v) => v.newtype_variant().map(LoroValue::I64),
            (LoroValueFields::String, v) => v
                .newtype_variant()
                .map(|x: String| LoroValue::String(x.into())),
            (LoroValueFields::List, v) => v.newtype_variant_seed(LoroCollectionSeed {
                seed: self.0,
                is_map: false,
            }),
            (LoroValueFields::Map, v) => v.newtype_variant_seed(LoroCollectionSeed {
                seed: self.0,
                is_map: true,
            }),
            (LoroValueFields::Container, v) => v.newtype_variant().map(LoroValue::Container),
            (LoroValueFields::Binary, v) => v
                .newtype_variant()
                .map(|x: Vec<u8>| LoroValue::Binary(x.into())),
        }
    }
}

pub fn to_value<T: Into<LoroValue>>(value: T) -> LoroValue {
    value.into()
}

#[cfg(feature = "serde_json")]
mod serde_json_impl {
    use serde_json::{Number, Value};

    use super::LoroValue;

    impl From<Value> for LoroValue {
        fn from(value: Value) -> Self {
            match value {
                Value::Null => LoroValue::Null,
                Value::Bool(b) => LoroValue::Bool(b),
                Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        LoroValue::I64(i)
                    } else {
                        LoroValue::Double(n.as_f64().unwrap())
                    }
                }
                Value::String(s) => LoroValue::String(s.into()),
                Value::Array(arr) => {
                    LoroValue::List(arr.into_iter().map(LoroValue::from).collect())
                }
                Value::Object(obj) => LoroValue::Map(
                    obj.into_iter()
                        .map(|(k, v)| (k, LoroValue::from(v)))
                        .collect(),
                ),
            }
        }
    }

    use super::LORO_CONTAINER_ID_PREFIX;
    impl From<LoroValue> for Value {
        fn from(value: LoroValue) -> Self {
            match value {
                LoroValue::Null => Value::Null,
                LoroValue::Bool(b) => Value::Bool(b),
                LoroValue::Double(d) => Number::from_f64(d).map_or(Value::Null, Value::Number),
                LoroValue::I64(i) => Value::Number(Number::from(i)),
                LoroValue::String(s) => Value::String(s.to_string()),
                LoroValue::List(l) => Value::Array(l.iter().cloned().map(Value::from).collect()),
                LoroValue::Map(m) => Value::Object(
                    m.iter()
                        .map(|(k, v)| (k.clone(), Value::from(v.clone())))
                        .collect(),
                ),
                LoroValue::Container(id) => {
                    Value::String(format!("{LORO_CONTAINER_ID_PREFIX}{id}"))
                }
                LoroValue::Binary(b) => Value::Array(b.iter().copied().map(Value::from).collect()),
            }
        }
    }
}

#[cfg(test)]
mod stack_safety_tests {
    use super::*;

    fn nested(depth: usize) -> LoroValue {
        let mut value = LoroValue::I64(0);
        for i in 0..depth {
            value = if i % 2 == 0 {
                LoroValue::Map(vec![("a".into(), value)].into())
            } else {
                LoroValue::List(vec![value].into())
            };
        }
        value
    }

    #[test]
    fn into_arc_preserves_the_existing_list_allocation() {
        let list = LoroListValue::from(vec![LoroValue::I64(42)]);
        let allocation = Arc::as_ptr(&list.0);
        let arc = list.into_arc();
        assert_eq!(Arc::as_ptr(&arc), allocation);
        assert_eq!(arc.as_slice(), &[LoroValue::I64(42)]);

        let list = LoroListValue::from(vec![LoroValue::I64(7)]);
        let shared = list.clone();
        let arc = list.into_arc();
        assert!(Arc::ptr_eq(&arc, &shared.into_arc()));
    }

    #[test]
    fn drop_deep_values_with_shared_children_and_unwrap() {
        std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(|| {
                let value = nested(100_000);
                let shared = value.clone();
                let wrapper = LoroListValue::from(vec![value]);
                drop(wrapper.unwrap());
                assert_eq!(shared.get_depth(), 100_000);
                let wrapper = LoroMapValue::from(vec![("value".into(), shared)]);
                drop(wrapper.unwrap());

                let value = nested(20_000);
                let clones: Vec<_> = (0..4).map(|_| value.clone()).collect();
                std::thread::scope(|scope| {
                    for clone in clones {
                        scope.spawn(move || drop(clone));
                    }
                    drop(value);
                });
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[cfg(feature = "serde_json")]
    #[test]
    fn serde_json_depth_limit_has_readable_error() {
        // Deserialize from an already built JSON value to bypass serde_json's
        // own text-parser recursion limit and exercise LoroValue's limit.
        let mut value = serde_json::Value::Null;
        for _ in 0..129 {
            value = serde_json::Value::Array(vec![value]);
        }
        let error = LoroValue::deserialize(&value).unwrap_err();
        assert!(error
            .to_string()
            .contains("LoroValue nesting depth exceeds 128"));
    }
}
