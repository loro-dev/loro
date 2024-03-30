use std::sync::Arc;

use js_sys::{Array, Object, Reflect, Uint8Array};
use loro_internal::delta::{DeltaItem, ResolvedMapDelta};
use loro_internal::event::Diff;
use loro_internal::handler::{Handler, ValueOrHandler};
use loro_internal::{LoroDoc, LoroValue};
use wasm_bindgen::JsValue;

use crate::{Container, JsContainer, LoroList, LoroMap, LoroText, LoroTree};
use wasm_bindgen::__rt::IntoJsResult;
use wasm_bindgen::convert::RefFromWasmAbi;

/// Convert a `JsValue` to `T` by constructor's name.
///
/// more details can be found in https://github.com/rustwasm/wasm-bindgen/issues/2231#issuecomment-656293288
pub(crate) fn js_to_container(js: JsContainer) -> Result<Container, JsValue> {
    let js: JsValue = js.into();
    if !js.is_object() {
        return Err(JsValue::from_str(&format!(
            "Value supplied is not an object, but {:?}",
            js
        )));
    }

    let kind_method = Reflect::get(&js, &JsValue::from_str("kind"));
    let kind = match kind_method {
        Ok(kind_method) if kind_method.is_function() => {
            let kind_string = js_sys::Function::from(kind_method).call0(&js);
            match kind_string {
                Ok(kind_string) if kind_string.is_string() => kind_string.as_string().unwrap(),
                _ => return Err(JsValue::from_str("kind() did not return a string")),
            }
        }
        _ => return Err(JsValue::from_str("No kind method found or not a function")),
    };

    let Ok(ptr) = Reflect::get(&js, &JsValue::from_str("__wbg_ptr")) else {
        return Err(JsValue::from_str("Cannot find pointer field"));
    };
    let ptr_u32: u32 = ptr.as_f64().unwrap() as u32;
    let container = match kind.as_str() {
        "Text" => {
            let obj = unsafe { LoroText::ref_from_abi(ptr_u32) };
            Container::Text(obj.clone())
        }
        "Map" => {
            let obj = unsafe { LoroMap::ref_from_abi(ptr_u32) };
            Container::Map(obj.clone())
        }
        "List" => {
            let obj = unsafe { LoroList::ref_from_abi(ptr_u32) };
            Container::List(obj.clone())
        }
        "Tree" => {
            let obj = unsafe { LoroTree::ref_from_abi(ptr_u32) };
            Container::Tree(obj.clone())
        }
        _ => {
            return Err(JsValue::from_str(
                format!(
                    "Value kind is {} but the valid container name is Map, List, Text or Tree",
                    kind
                )
                .as_str(),
            ));
        }
    };

    Ok(container)
}

pub(crate) fn resolved_diff_to_js(value: &Diff, doc: &Arc<LoroDoc>) -> JsValue {
    // create a obj
    let obj = Object::new();
    match value {
        Diff::Tree(tree) => {
            js_sys::Reflect::set(&obj, &JsValue::from_str("type"), &JsValue::from_str("tree"))
                .unwrap();
            js_sys::Reflect::set(&obj, &JsValue::from_str("diff"), &tree.into()).unwrap();
        }
        Diff::List(list) => {
            // set type as "list"
            js_sys::Reflect::set(&obj, &JsValue::from_str("type"), &JsValue::from_str("list"))
                .unwrap();
            // set diff as array
            let arr = Array::new_with_length(list.len() as u32);
            for (i, v) in list.iter().enumerate() {
                arr.set(i as u32, delta_item_to_js(v.clone(), doc));
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
            js_sys::Reflect::set(&obj, &JsValue::from_str("type"), &JsValue::from_str("text"))
                .unwrap();
            // set diff as array
            js_sys::Reflect::set(&obj, &JsValue::from_str("diff"), &JsValue::from(text)).unwrap();
        }
        Diff::Map(map) => {
            js_sys::Reflect::set(&obj, &JsValue::from_str("type"), &JsValue::from_str("map"))
                .unwrap();

            js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("updated"),
                &map_delta_to_js(map, doc),
            )
            .unwrap();
        }
        _ => unreachable!(),
    };

    // convert object to js value
    obj.into_js_result().unwrap()
}

fn delta_item_to_js(item: DeltaItem<Vec<ValueOrHandler>, ()>, doc: &Arc<LoroDoc>) -> JsValue {
    let obj = Object::new();
    match item {
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
                let value = match v {
                    ValueOrHandler::Value(v) => convert(v),
                    ValueOrHandler::Handler(h) => handler_to_js_value(h, Some(doc.clone())),
                };
                arr.set(i as u32, value);
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

pub fn convert(value: LoroValue) -> JsValue {
    match value {
        LoroValue::Null => JsValue::NULL,
        LoroValue::Bool(b) => JsValue::from_bool(b),
        LoroValue::Double(f) => JsValue::from_f64(f),
        LoroValue::I64(i) => JsValue::from_f64(i as f64),
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
        LoroValue::Container(container_id) => JsValue::from(&container_id),
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

fn map_delta_to_js(value: &ResolvedMapDelta, doc: &Arc<LoroDoc>) -> JsValue {
    let obj = Object::new();
    for (key, value) in value.updated.iter() {
        let value = if let Some(value) = value.value.clone() {
            match value {
                ValueOrHandler::Value(v) => convert(v),
                ValueOrHandler::Handler(h) => handler_to_js_value(h, Some(doc.clone())),
            }
        } else {
            JsValue::null()
        };

        js_sys::Reflect::set(&obj, &JsValue::from_str(key), &value).unwrap();
    }

    obj.into_js_result().unwrap()
}

pub(crate) fn handler_to_js_value(handler: Handler, doc: Option<Arc<LoroDoc>>) -> JsValue {
    match handler {
        Handler::Text(t) => LoroText { handler: t, doc }.into(),
        Handler::Map(m) => LoroMap { handler: m, doc }.into(),
        Handler::List(l) => LoroList { handler: l, doc }.into(),
        Handler::Tree(t) => LoroTree { handler: t, doc }.into(),
    }
}
