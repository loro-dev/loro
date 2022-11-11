use std::collections::HashMap;

use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use serde::{ser::SerializeStruct, Deserialize, Serialize};

use crate::{container::ContainerID, context::Context, Container};

/// [LoroValue] is used to represents the state of CRDT at a given version
#[derive(Debug, PartialEq, Clone, EnumAsInner)]
pub enum LoroValue {
    Null,
    Bool(bool),
    Double(f64),
    I32(i32),
    // i64?
    String(Box<str>),
    List(Box<Vec<LoroValue>>),
    Map(Box<FxHashMap<String, LoroValue>>),
    Unresolved(Box<ContainerID>),
}

#[derive(Serialize, Deserialize)]
enum Test {
    Unknown(ContainerID),
    Map(FxHashMap<u32, usize>),
}

impl Serialize for LoroValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            LoroValue::Null => serializer.serialize_none(),
            LoroValue::Bool(b) => serializer.serialize_bool(*b),
            LoroValue::Double(d) => serializer.serialize_f64(*d),
            LoroValue::I32(i) => serializer.serialize_i32(*i),
            LoroValue::String(s) => serializer.serialize_str(s),
            LoroValue::List(l) => serializer.collect_seq(l.iter()),
            LoroValue::Map(m) => serializer.collect_map(m.iter()),
            LoroValue::Unresolved(id) => {
                let mut state = serializer.serialize_struct("Unresolved", 1)?;
                state.serialize_field("Unresolved", id)?;
                state.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for LoroValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct LoroValueVisitor;

        impl<'de> serde::de::Visitor<'de> for LoroValueVisitor {
            type Value = LoroValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a LoroValue")
            }

            fn visit_none<E>(self) -> Result<Self::Value, E>
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
                Ok(LoroValue::String(v.into()))
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

        deserializer.deserialize_any(LoroValueVisitor)
    }
}

impl LoroValue {
    pub(crate) fn resolve<C: Context>(&self, ctx: &C) -> Option<LoroValue> {
        if let Some(id) = self.as_unresolved() {
            ctx.get_container(id)
                .map(|container| container.lock().unwrap().get_value())
        } else {
            None
        }
    }

    #[cfg(feature = "json")]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    #[cfg(feature = "json")]
    pub fn from_json(s: &str) -> Self {
        serde_json::from_str(s).unwrap()
    }
}

impl Default for LoroValue {
    fn default() -> Self {
        LoroValue::Null
    }
}

impl<S: Into<String>, M> From<HashMap<S, LoroValue, M>> for LoroValue {
    fn from(map: HashMap<S, LoroValue, M>) -> Self {
        let mut new_map = FxHashMap::default();
        for (k, v) in map {
            new_map.insert(k.into(), v);
        }

        LoroValue::Map(Box::new(new_map))
    }
}

impl From<Vec<LoroValue>> for LoroValue {
    fn from(vec: Vec<LoroValue>) -> Self {
        LoroValue::List(Box::new(vec))
    }
}

impl From<InsertValue> for LoroValue {
    fn from(v: InsertValue) -> Self {
        match v {
            InsertValue::Null => LoroValue::Null,
            InsertValue::Bool(b) => LoroValue::Bool(b),
            InsertValue::Double(d) => LoroValue::Double(d),
            InsertValue::Int32(i) => LoroValue::I32(i),
            InsertValue::String(s) => LoroValue::String(s),
            InsertValue::Container(c) => LoroValue::Unresolved(c),
        }
    }
}

impl From<LoroValue> for InsertValue {
    fn from(v: LoroValue) -> Self {
        match v {
            LoroValue::Null => InsertValue::Null,
            LoroValue::Bool(b) => InsertValue::Bool(b),
            LoroValue::Double(d) => InsertValue::Double(d),
            LoroValue::I32(i) => InsertValue::Int32(i),
            LoroValue::String(s) => InsertValue::String(s),
            LoroValue::Unresolved(c) => InsertValue::Container(c),
            _ => unreachable!("Unsupported convert from LoroValue to InsertValue"),
        }
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
        LoroValue::String(v.into())
    }
}

impl From<String> for LoroValue {
    fn from(v: String) -> Self {
        LoroValue::String(v.into())
    }
}

/// [InsertValue] can be inserted to Map or List
/// It's different from [LoroValue] because some of the states in [LoroValue] are illegal to be inserted
#[derive(Debug, PartialEq, Clone, EnumAsInner)]
pub enum InsertValue {
    Null,
    Bool(bool),
    Double(f64),
    Int32(i32),
    String(Box<str>),
    Container(Box<ContainerID>),
}

#[cfg(feature = "wasm")]
pub mod wasm {
    use js_sys::{Array, Object};
    use wasm_bindgen::{JsValue, __rt::IntoJsResult};

    use crate::LoroValue;

    use super::InsertValue;

    pub fn convert(value: LoroValue) -> JsValue {
        match value {
            LoroValue::Null => JsValue::NULL,
            LoroValue::Bool(b) => JsValue::from_bool(b),
            LoroValue::Double(f) => JsValue::from_f64(f),
            LoroValue::I32(i) => JsValue::from_f64(i as f64),
            LoroValue::String(s) => JsValue::from_str(&s),
            LoroValue::List(list) => {
                let arr = Array::new_with_length(list.len() as u32);
                for v in list.into_iter() {
                    arr.push(&convert(v));
                }

                arr.into_js_result().unwrap()
            }
            LoroValue::Map(m) => {
                let map = Object::new();
                for (k, v) in m.into_iter() {
                    let str: &str = &k;
                    js_sys::Reflect::set(&map, &JsValue::from_str(str), &convert(v)).unwrap();
                }

                map.into_js_result().unwrap()
            }
            LoroValue::Unresolved(_) => {
                unreachable!()
            }
        }
    }

    impl From<LoroValue> for JsValue {
        fn from(value: LoroValue) -> Self {
            convert(value)
        }
    }

    impl InsertValue {
        pub fn try_from_js(value: JsValue) -> Result<InsertValue, JsValue> {
            if value.is_null() {
                Ok(InsertValue::Null)
            } else if value.as_bool().is_some() {
                Ok(InsertValue::Bool(value.as_bool().unwrap()))
            } else if value.as_f64().is_some() {
                Ok(InsertValue::Double(value.as_f64().unwrap()))
            } else if value.is_string() {
                Ok(InsertValue::String(value.as_string().unwrap().into()))
            } else {
                Err(value)
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod proptest {
    use proptest::prelude::*;
    use proptest::prop_oneof;

    use super::InsertValue;

    pub fn gen_insert_value() -> impl Strategy<Value = InsertValue> {
        prop_oneof![
            Just(InsertValue::Null),
            any::<f64>().prop_map(InsertValue::Double),
            any::<i32>().prop_map(InsertValue::Int32),
            any::<bool>().prop_map(InsertValue::Bool),
            any::<String>().prop_map(|s| InsertValue::String(s.into())),
        ]
    }
}

#[cfg(test)]
#[cfg(feature = "json")]
mod json_test {
    use crate::{fx_map, LoroValue};
    use fxhash::FxHashMap;

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
