use std::collections::HashMap;

use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::{container::ContainerID, context::Context, Container};

/// [LoroValue] is used to represents the state of CRDT at a given version
#[derive(Debug, PartialEq, Clone, EnumAsInner, Serialize, Deserialize)]
#[serde(untagged)]
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

impl LoroValue {
    pub(crate) fn resolve_deep<C: Context>(&self, ctx: &C) -> Option<LoroValue> {
        if let Some(id) = self.as_unresolved() {
            ctx.get_container(id).map(|container| {
                let mut value = container.lock().unwrap().get_value();
                match &mut value {
                    LoroValue::List(list) => {
                        for v in list.iter_mut() {
                            if v.as_unresolved().is_some() {
                                *v = v.resolve_deep(ctx).unwrap();
                            }
                        }
                    }
                    LoroValue::Map(map) => {
                        for v in map.values_mut() {
                            if v.as_unresolved().is_some() {
                                *v = v.resolve_deep(ctx).unwrap();
                            }
                        }
                    }
                    LoroValue::Unresolved(_) => unreachable!(),
                    _ => {}
                }
                value
            })
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

impl From<ContainerID> for LoroValue {
    fn from(v: ContainerID) -> Self {
        LoroValue::Unresolved(Box::new(v))
    }
}

#[cfg(feature = "wasm")]
pub mod wasm {
    use fxhash::FxHashMap;
    use js_sys::{Array, Object};
    use wasm_bindgen::{JsCast, JsValue, __rt::IntoJsResult};

    use crate::LoroValue;

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
                LoroValue::String(js_value.as_string().unwrap().into_boxed_str())
            } else if js_value.has_type::<Array>() {
                let array = js_value.unchecked_into::<Array>();
                let mut list = Vec::new();
                for i in 0..array.length() {
                    list.push(LoroValue::from(array.get(i)));
                }

                LoroValue::List(Box::new(list))
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
