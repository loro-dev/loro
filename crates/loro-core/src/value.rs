use enum_as_inner::EnumAsInner;
use fxhash::FxHashMap;

use crate::{container::ContainerID, smstring::SmString, InternalString};

/// [LoroValue] is used to represents the state of CRDT at a given version
#[derive(Debug, PartialEq, Clone, serde::Serialize, EnumAsInner)]
pub enum LoroValue {
    Null,
    Bool(bool),
    Double(f64),
    Integer(i32),
    String(SmString),
    List(Vec<LoroValue>),
    Map(FxHashMap<InternalString, LoroValue>),
    Unresolved(ContainerID),
}

impl Default for LoroValue {
    fn default() -> Self {
        LoroValue::Null
    }
}

impl From<InsertValue> for LoroValue {
    fn from(v: InsertValue) -> Self {
        match v {
            InsertValue::Null => LoroValue::Null,
            InsertValue::Bool(b) => LoroValue::Bool(b),
            InsertValue::Double(d) => LoroValue::Double(d),
            InsertValue::Int32(i) => LoroValue::Integer(i),
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
            LoroValue::Integer(i) => InsertValue::Int32(i),
            LoroValue::String(s) => InsertValue::String(s),
            LoroValue::Unresolved(c) => InsertValue::Container(c),
            _ => unreachable!("Unsupported convert from LoroValue to InsertValue"),
        }
    }
}

/// [InsertValue] can be inserted to Map or List
/// It's different from [LoroValue] because some of the states in [LoroValue] are illegal to be inserted
#[derive(Debug, PartialEq, Clone)]
pub enum InsertValue {
    Null,
    Bool(bool),
    Double(f64),
    Int32(i32),
    String(SmString),
    Container(ContainerID),
}

#[cfg(feature = "wasm")]
pub mod wasm {
    use js_sys::{Array, Object};
    use wasm_bindgen::{JsValue, __rt::IntoJsResult};

    use crate::LoroValue;

    pub fn convert(value: LoroValue) -> JsValue {
        match value {
            LoroValue::Null => JsValue::NULL,
            LoroValue::Bool(b) => JsValue::from_bool(b),
            LoroValue::Double(f) => JsValue::from_f64(f),
            LoroValue::Integer(i) => JsValue::from_f64(i as f64),
            LoroValue::String(s) => JsValue::from_str(&s),
            LoroValue::List(list) => {
                let arr = Array::new_with_length(list.len() as u32);
                for v in list {
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
