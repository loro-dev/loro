use std::sync::Arc;

use js_sys::{Array, Map, Object, Reflect, Uint8Array};
use loro_internal::container::ContainerID;
use loro_internal::delta::ResolvedMapDelta;
use loro_internal::encoding::{ImportBlobMetadata, ImportStatus};
use loro_internal::event::Diff;
use loro_internal::handler::{Handler, ValueOrHandler};
use loro_internal::undo::DiffBatch;
use loro_internal::version::VersionRange;
use loro_internal::{Counter, CounterSpan, FxHashMap, IdSpan, ListDiffItem, LoroDoc, LoroValue};
use wasm_bindgen::{JsCast, JsValue};

use crate::{
    frontiers_to_ids, Container, Cursor, JsContainer, JsIdSpan, JsImportBlobMetadata, JsResult,
    LoroCounter, LoroList, LoroMap, LoroMovableList, LoroText, LoroTree, VersionVector,
};
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
        "MovableList" => {
            let obj = unsafe { LoroMovableList::ref_from_abi(ptr_u32) };
            Container::MovableList(obj.clone())
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

pub(crate) fn js_to_id_span(js: JsIdSpan) -> Result<IdSpan, JsValue> {
    let value: JsValue = js.into();
    let peer = Reflect::get(&value, &JsValue::from_str("peer"))?
        .as_string()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    let counter = Reflect::get(&value, &JsValue::from_str("counter"))?
        .as_f64()
        .unwrap() as Counter;
    let length = Reflect::get(&value, &JsValue::from_str("length"))?
        .as_f64()
        .unwrap() as Counter;
    Ok(IdSpan::new(peer, counter, counter + length))
}

pub(crate) fn js_to_version_vector(
    js: JsValue,
) -> Result<wasm_bindgen::__rt::Ref<'static, VersionVector>, JsValue> {
    if !js.is_object() {
        return Err(JsValue::from_str(&format!(
            "Value supplied is not an object, but {:?}",
            js
        )));
    }

    if js.is_null() || js.is_undefined() {
        return Err(JsValue::from_str(&format!(
            "Value supplied is not an object, but {:?}",
            js
        )));
    }

    if !js.is_object() {
        return Err(JsValue::from_str("Expected an object or Uint8Array"));
    }

    let Ok(ptr) = Reflect::get(&js, &JsValue::from_str("__wbg_ptr")) else {
        return Err(JsValue::from_str("Cannot find pointer field"));
    };

    let ptr_u32: u32 = ptr.as_f64().unwrap() as u32;
    let vv = unsafe { VersionVector::ref_from_abi(ptr_u32) };
    Ok(vv)
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
            let arr = Array::new();
            let mut i = 0;
            for v in list.iter() {
                let (a, b) = delta_item_to_js(v.clone(), doc);
                arr.set(i as u32, a);
                i += 1;
                if let Some(b) = b {
                    arr.set(i as u32, b);
                    i += 1;
                }
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
            js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("diff"),
                &loro_internal::wasm::text_diff_to_js_value(text),
            )
            .unwrap();
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

        Diff::Counter(v) => {
            js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("type"),
                &JsValue::from_str("counter"),
            )
            .unwrap();
            js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("increment"),
                &JsValue::from_f64(*v),
            )
            .unwrap();
        }
        _ => unreachable!(),
    };

    // convert object to js value
    obj.into_js_result().unwrap()
}

pub(crate) fn js_diff_to_inner_diff(js: JsValue) -> JsResult<Diff> {
    let obj = js.dyn_into::<Object>()?;
    let diff_type = js_sys::Reflect::get(&obj, &"type".into())?;
    let diff_type = diff_type.as_string().ok_or("type must be string")?;

    match diff_type.as_str() {
        "text" => {
            let diff = js_sys::Reflect::get(&obj, &"diff".into())?;
            let text_diff = loro_internal::wasm::js_value_to_text_diff(diff)?;
            Ok(Diff::Text(text_diff))
        }
        "map" => {
            let updated = js_sys::Reflect::get(&obj, &"updated".into())?;
            let map_diff = js_to_map_delta(updated)?;
            Ok(Diff::Map(map_diff))
        }
        "counter" => {
            let increment = js_sys::Reflect::get(&obj, &"increment".into())?;
            let increment = increment.as_f64().ok_or("increment must be number")?;
            Ok(Diff::Counter(increment))
        }
        "tree" => {
            let diff = js_sys::Reflect::get(&obj, &"diff".into())?;
            let tree_diff = (&diff).try_into()?;
            Ok(Diff::Tree(tree_diff))
        }
        "list" => {
            let diff = js_sys::Reflect::get(&obj, &"diff".into())?;
            let list_diff = loro_internal::wasm::js_value_to_tree_diff(diff)?;
            Ok(Diff::List(list_diff))
        }
        _ => Err(format!("Unknown diff type: {}", diff_type).into()),
    }
}

fn delta_item_to_js(item: ListDiffItem, doc: &Arc<LoroDoc>) -> (JsValue, Option<JsValue>) {
    match item {
        loro_internal::loro_delta::DeltaItem::Retain { len, attr: _ } => {
            let obj = Object::new();
            js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("retain"),
                &JsValue::from_f64(len as f64),
            )
            .unwrap();
            (obj.into_js_result().unwrap(), None)
        }
        loro_internal::loro_delta::DeltaItem::Replace {
            value,
            attr: _,
            delete,
        } => {
            let mut a = None;
            let mut b: Option<JsValue> = None;
            if value.len() > 0 {
                let obj = Object::new();
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
                a = Some(obj.into_js_result().unwrap());
            }
            if delete > 0 {
                let obj = Object::new();
                js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str("delete"),
                    &JsValue::from_f64(delete as f64),
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

pub(crate) fn js_to_cursor(js: JsValue) -> Result<Cursor, JsValue> {
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

    if kind.as_str() != "Cursor" {
        return Err(JsValue::from_str("Value is not a Cursor"));
    }

    let Ok(ptr) = Reflect::get(&js, &JsValue::from_str("__wbg_ptr")) else {
        return Err(JsValue::from_str("Cannot find pointer field"));
    };
    let ptr_u32: u32 = ptr.as_f64().unwrap() as u32;
    let cursor = unsafe { Cursor::ref_from_abi(ptr_u32) };
    Ok(cursor.clone())
}

pub fn convert(value: LoroValue) -> JsValue {
    match value {
        LoroValue::Null => JsValue::NULL,
        LoroValue::Bool(b) => JsValue::from_bool(b),
        LoroValue::Double(f) => JsValue::from_f64(f),
        LoroValue::I64(i) => JsValue::from_f64(i as f64),
        LoroValue::String(s) => JsValue::from_str(&s),
        LoroValue::List(list) => {
            let list = list.unwrap();
            let arr = Array::new_with_length(list.len() as u32);
            for (i, v) in list.into_iter().enumerate() {
                arr.set(i as u32, convert(v));
            }
            arr.into_js_result().unwrap()
        }
        LoroValue::Map(m) => {
            let m = m.unwrap();
            let map = Object::new();
            for (k, v) in m.into_iter() {
                let str: &str = &k;
                js_sys::Reflect::set(&map, &JsValue::from_str(str), &convert(v)).unwrap();
            }

            map.into_js_result().unwrap()
        }
        LoroValue::Container(container_id) => JsValue::from(&container_id),
        LoroValue::Binary(binary) => {
            let binary = binary.unwrap();
            let arr = Uint8Array::new_with_length(binary.len() as u32);
            for (i, v) in binary.into_iter().enumerate() {
                arr.set_index(i as u32, v);
            }
            arr.into_js_result().unwrap()
        }
    }
}

impl From<ImportBlobMetadata> for JsImportBlobMetadata {
    fn from(meta: ImportBlobMetadata) -> Self {
        let start_vv = super::VersionVector(meta.partial_start_vv);
        let end_vv = super::VersionVector(meta.partial_end_vv);
        let start_vv: JsValue = start_vv.into();
        let end_vv: JsValue = end_vv.into();
        let start_timestamp: JsValue = JsValue::from_f64(meta.start_timestamp as f64);
        let end_timestamp: JsValue = JsValue::from_f64(meta.end_timestamp as f64);
        let mode: JsValue = JsValue::from_str(&meta.mode.to_string());
        let change_num: JsValue = JsValue::from_f64(meta.change_num as f64);
        let ans = Object::new();
        js_sys::Reflect::set(
            &ans,
            &JsValue::from_str("partialStartVersionVector"),
            &start_vv,
        )
        .unwrap();
        js_sys::Reflect::set(&ans, &JsValue::from_str("partialEndVersionVector"), &end_vv).unwrap();
        let js_frontiers: JsValue = frontiers_to_ids(&meta.start_frontiers).into();
        js_sys::Reflect::set(&ans, &JsValue::from_str("startFrontiers"), &js_frontiers).unwrap();
        js_sys::Reflect::set(&ans, &JsValue::from_str("startTimestamp"), &start_timestamp).unwrap();
        js_sys::Reflect::set(&ans, &JsValue::from_str("endTimestamp"), &end_timestamp).unwrap();
        js_sys::Reflect::set(&ans, &JsValue::from_str("mode"), &mode).unwrap();
        js_sys::Reflect::set(&ans, &JsValue::from_str("changeNum"), &change_num).unwrap();
        let ans: JsValue = ans.into();
        ans.into()
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
        Handler::Text(t) => LoroText {
            handler: t,
            doc,
            delta_cache: None,
        }
        .into(),
        Handler::Map(m) => LoroMap { handler: m, doc }.into(),
        Handler::List(l) => LoroList { handler: l, doc }.into(),
        Handler::Tree(t) => LoroTree { handler: t, doc }.into(),
        Handler::MovableList(m) => LoroMovableList { handler: m, doc }.into(),
        Handler::Counter(c) => LoroCounter { handler: c, doc }.into(),
        Handler::Unknown(_) => unreachable!(),
    }
}

pub(crate) fn import_status_to_js_value(status: ImportStatus) -> JsValue {
    let obj = Object::new();
    js_sys::Reflect::set(
        &obj,
        &JsValue::from_str("success"),
        &id_span_vector_to_js_value(status.success),
    )
    .unwrap();
    js_sys::Reflect::set(
        &obj,
        &JsValue::from_str("pending"),
        &match status.pending {
            None => JsValue::null(),
            Some(pending) => id_span_vector_to_js_value(pending),
        },
    )
    .unwrap();
    obj.into()
}

fn id_span_vector_to_js_value(v: VersionRange) -> JsValue {
    let map = Map::new();
    for (k, v) in v.iter() {
        Map::set(
            &map,
            &JsValue::from_str(&k.to_string()),
            &JsValue::from(CounterSpan {
                start: v.0,
                end: v.1,
            }),
        );
    }
    map.into()
}
