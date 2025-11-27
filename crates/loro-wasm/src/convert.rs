use js_sys::{Array, Map, Object, Reflect, Uint8Array};
use loro_common::{ContainerID, IdLp, LoroListValue, LoroMapValue, LoroValue};
use loro_delta::{array_vec, DeltaRopeBuilder};
use loro_internal::delta::{ResolvedMapDelta, ResolvedMapValue};
use loro_internal::encoding::{ImportBlobMetadata, ImportStatus};
use loro_internal::event::{Diff, ListDeltaMeta, ListDiff, TextDiff, TextMeta};
use loro_internal::handler::{Handler, TextDelta, ValueOrHandler};
use loro_internal::json::JsonSchema;
use loro_internal::version::VersionRange;
use loro_internal::StringSlice;
use loro_internal::{Counter, CounterSpan, FxHashMap, IdSpan, ListDiffItem};
use serde::Serialize;
use wasm_bindgen::{JsCast, JsValue};

use crate::{
    frontiers_to_ids, Container, Cursor, JsContainer, JsIdSpan, JsImportBlobMetadata, JsJsonSchema,
    JsJsonSchemaOrString, JsResult, LoroCounter, LoroList, LoroMap, LoroMovableList, LoroText,
    LoroTree, VersionVector,
};
use wasm_bindgen::__rt::IntoJsResult;
use wasm_bindgen::convert::RefFromWasmAbi;

/// Convert a `JsValue` to `T` by constructor's name.
///
/// more details can be found in https://github.com/rustwasm/wasm-bindgen/issues/2231#issuecomment-656293288
pub(crate) fn js_to_container(js: JsContainer) -> JsResult<Container> {
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
                Ok(kind_string) if kind_string.is_string() => kind_string
                    .as_string()
                    .ok_or_else(|| JsValue::from_str("kind() did not return a string"))?,
                _ => return Err(JsValue::from_str("kind() did not return a string")),
            }
        }
        _ => return Err(JsValue::from_str("No kind method found or not a function")),
    };

    let Ok(ptr) = Reflect::get(&js, &JsValue::from_str("__wbg_ptr")) else {
        return Err(JsValue::from_str("Cannot find pointer field"));
    };
    let ptr_u32: u32 =
        ptr.as_f64()
            .ok_or_else(|| JsValue::from_str("__wbg_ptr must be a number"))? as u32;
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
        "Counter" => {
            let obj = unsafe { LoroCounter::ref_from_abi(ptr_u32) };
            Container::Counter(obj.clone())
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

pub(crate) fn js_to_id_span(js: JsIdSpan) -> JsResult<IdSpan> {
    let value: JsValue = js.into();
    let peer = Reflect::get(&value, &JsValue::from_str("peer"))?
        .as_string()
        .ok_or_else(|| JsValue::from_str("peer must be a string"))?
        .parse::<u64>()
        .map_err(|_| JsValue::from_str("peer must be a number"))?;
    let counter = Reflect::get(&value, &JsValue::from_str("counter"))?
        .as_f64()
        .ok_or_else(|| JsValue::from_str("counter must be a number"))? as Counter;
    let length = Reflect::get(&value, &JsValue::from_str("length"))?
        .as_f64()
        .ok_or_else(|| JsValue::from_str("length must be a number"))? as Counter;
    Ok(IdSpan::new(peer, counter, counter + length))
}

pub(crate) fn js_to_version_vector(
    js: JsValue,
) -> JsResult<wasm_bindgen::__rt::RcRef<VersionVector>> {
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

    let ptr_u32: u32 =
        ptr.as_f64()
            .ok_or_else(|| JsValue::from_str("__wbg_ptr must be a number"))? as u32;
    let vv = unsafe { VersionVector::ref_from_abi(ptr_u32) };
    Ok(vv)
}

pub(crate) fn resolved_diff_to_js(value: &Diff, for_json: bool) -> JsResult<JsValue> {
    // create a obj
    let obj = Object::new();
    match value {
        Diff::Tree(tree) => {
            js_sys::Reflect::set(&obj, &JsValue::from_str("type"), &JsValue::from_str("tree"))?;
            js_sys::Reflect::set(&obj, &JsValue::from_str("diff"), &tree.into())?;
        }
        Diff::List(list) => {
            // set type as "list"
            js_sys::Reflect::set(&obj, &JsValue::from_str("type"), &JsValue::from_str("list"))?;
            // set diff as array
            let arr = Array::new();
            let mut i = 0;
            for v in list.iter() {
                let (a, b) = delta_item_to_js(v.clone(), for_json)?;
                arr.set(i as u32, a);
                i += 1;
                if let Some(b) = b {
                    arr.set(i as u32, b);
                    i += 1;
                }
            }
            let diff = arr.into_js_result()?;
            js_sys::Reflect::set(&obj, &JsValue::from_str("diff"), &diff)?;
        }
        Diff::Text(text) => {
            // set type as "text"
            js_sys::Reflect::set(&obj, &JsValue::from_str("type"), &JsValue::from_str("text"))?;
            // set diff as array
            js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("diff"),
                &loro_internal::wasm::text_diff_to_js_value(text),
            )?;
        }
        Diff::Map(map) => {
            js_sys::Reflect::set(&obj, &JsValue::from_str("type"), &JsValue::from_str("map"))?;

            js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("updated"),
                &map_delta_to_js(map, for_json)?,
            )?;
        }

        Diff::Counter(v) => {
            js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("type"),
                &JsValue::from_str("counter"),
            )?;
            js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("increment"),
                &JsValue::from_f64(*v),
            )?;
        }
        _ => unreachable!(),
    };

    // convert object to js value
    obj.into_js_result()
}

pub(crate) fn js_diff_to_inner_diff(js: JsValue) -> JsResult<Diff> {
    let obj = js.dyn_into::<Object>()?;
    let diff_type = js_sys::Reflect::get(&obj, &"type".into())?;
    let diff_type = diff_type.as_string().ok_or("type must be string")?;

    match diff_type.as_str() {
        "text" => {
            let diff = js_sys::Reflect::get(&obj, &"diff".into())?;
            let text_diff = js_value_to_text_diff(&diff)?;
            Ok(Diff::Text(text_diff))
        }
        "map" => {
            let updated = js_sys::Reflect::get(&obj, &"updated".into())?;
            let map_diff = js_to_map_delta(&updated)?;
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
            let list_diff = js_value_to_list_diff(&diff)?;
            Ok(Diff::List(list_diff))
        }
        _ => Err(format!("Unknown diff type: {diff_type}").into()),
    }
}

fn delta_item_to_js(item: ListDiffItem, for_json: bool) -> JsResult<(JsValue, Option<JsValue>)> {
    match item {
        loro_internal::loro_delta::DeltaItem::Retain { len, attr: _ } => {
            let obj = Object::new();
            js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("retain"),
                &JsValue::from_f64(len as f64),
            )?;
            Ok((obj.into_js_result()?, None))
        }
        loro_internal::loro_delta::DeltaItem::Replace {
            value,
            attr: _,
            delete,
        } => {
            let mut first = None;
            let mut second: Option<JsValue> = None;
            if !value.is_empty() {
                let obj = Object::new();
                let arr = Array::new_with_length(value.len() as u32);
                for (i, v) in value.into_iter().enumerate() {
                    let value = match v {
                        ValueOrHandler::Value(v) => convert(v)?,
                        ValueOrHandler::Handler(h) => handler_to_js_value(h, for_json),
                    };
                    arr.set(i as u32, value);
                }

                js_sys::Reflect::set(&obj, &JsValue::from_str("insert"), &arr.into_js_result()?)?;
                first = Some(obj.into_js_result()?);
            }
            if delete > 0 {
                let obj = Object::new();
                js_sys::Reflect::set(
                    &obj,
                    &JsValue::from_str("delete"),
                    &JsValue::from_f64(delete as f64),
                )?;
                if first.is_some() {
                    second = Some(obj.into_js_result()?);
                } else {
                    first = Some(obj.into_js_result()?);
                }
            }

            let first =
                first.ok_or_else(|| JsValue::from_str("Replace delta must insert or delete"))?;

            Ok((first, second))
        }
    }
}

pub(crate) fn js_to_cursor(js: JsValue) -> JsResult<Cursor> {
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
                Ok(kind_string) if kind_string.is_string() => kind_string
                    .as_string()
                    .ok_or_else(|| JsValue::from_str("kind() did not return a string"))?,
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
    let ptr_u32: u32 =
        ptr.as_f64()
            .ok_or_else(|| JsValue::from_str("__wbg_ptr must be a number"))? as u32;
    let cursor = unsafe { Cursor::ref_from_abi(ptr_u32) };
    Ok(cursor.clone())
}

pub(crate) fn convert(value: LoroValue) -> JsResult<JsValue> {
    Ok(match value {
        LoroValue::Null => JsValue::NULL,
        LoroValue::Bool(b) => JsValue::from_bool(b),
        LoroValue::Double(f) => JsValue::from_f64(f),
        LoroValue::I64(i) => JsValue::from_f64(i as f64),
        LoroValue::String(s) => JsValue::from_str(&s),
        LoroValue::List(list) => {
            let arr = Array::new_with_length(list.len() as u32);
            for (i, v) in list.as_ref().iter().cloned().enumerate() {
                arr.set(i as u32, convert(v)?);
            }
            arr.into_js_result()?
        }
        LoroValue::Map(m) => {
            let map = Object::new();
            for (k, v) in m.as_ref().iter() {
                let str: &str = k;
                js_sys::Reflect::set(&map, &JsValue::from_str(str), &convert(v.clone())?)?;
            }

            map.into_js_result()?
        }
        LoroValue::Container(container_id) => JsValue::from(&container_id),
        LoroValue::Binary(binary) => {
            let arr = Uint8Array::new_with_length(binary.len() as u32);
            for (i, v) in binary.iter().enumerate() {
                arr.set_index(i as u32, *v);
            }
            arr.into_js_result()?
        }
    })
}

impl From<ImportBlobMetadata> for JsImportBlobMetadata {
    fn from(meta: ImportBlobMetadata) -> Self {
        match import_blob_metadata_to_js(meta) {
            Ok(value) => value,
            Err(err) => wasm_bindgen::throw_val(err),
        }
    }
}

pub(crate) fn import_blob_metadata_to_js(
    meta: ImportBlobMetadata,
) -> JsResult<JsImportBlobMetadata> {
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
    )?;
    js_sys::Reflect::set(&ans, &JsValue::from_str("partialEndVersionVector"), &end_vv)?;
    let js_frontiers: JsValue = frontiers_to_ids(&meta.start_frontiers)?.into();
    js_sys::Reflect::set(&ans, &JsValue::from_str("startFrontiers"), &js_frontiers)?;
    js_sys::Reflect::set(&ans, &JsValue::from_str("startTimestamp"), &start_timestamp)?;
    js_sys::Reflect::set(&ans, &JsValue::from_str("endTimestamp"), &end_timestamp)?;
    js_sys::Reflect::set(&ans, &JsValue::from_str("mode"), &mode)?;
    js_sys::Reflect::set(&ans, &JsValue::from_str("changeNum"), &change_num)?;
    let ans: JsValue = ans.into();
    Ok(ans.into())
}

fn map_delta_to_js(value: &ResolvedMapDelta, for_json: bool) -> JsResult<JsValue> {
    let obj = Object::new();
    for (key, value) in value.updated.iter() {
        let value = if let Some(value) = value.value.clone() {
            match value {
                ValueOrHandler::Value(v) => convert(v)?,
                ValueOrHandler::Handler(h) => handler_to_js_value(h, for_json),
            }
        } else {
            JsValue::undefined()
        };

        js_sys::Reflect::set(&obj, &JsValue::from_str(key), &value)?;
    }

    obj.into_js_result()
}

pub(crate) fn handler_to_js_value(handler: Handler, for_json: bool) -> JsValue {
    if for_json {
        let cid = handler.id();
        return JsValue::from_str(&cid.to_loro_value_string());
    }

    match handler {
        Handler::Text(t) => LoroText { handler: t }.into(),
        Handler::Map(m) => LoroMap { handler: m }.into(),
        Handler::List(l) => LoroList { handler: l }.into(),
        Handler::Tree(t) => LoroTree { handler: t }.into(),
        Handler::MovableList(m) => LoroMovableList { handler: m }.into(),
        Handler::Counter(c) => LoroCounter { handler: c }.into(),
        Handler::Unknown(_) => unreachable!(),
    }
}

pub(crate) fn import_status_to_js_value(status: ImportStatus) -> JsResult<JsValue> {
    let obj = Object::new();
    js_sys::Reflect::set(
        &obj,
        &JsValue::from_str("success"),
        &id_span_vector_to_js_value(status.success),
    )?;
    js_sys::Reflect::set(
        &obj,
        &JsValue::from_str("pending"),
        &match status.pending {
            None => JsValue::null(),
            Some(pending) => id_span_vector_to_js_value(pending),
        },
    )?;
    Ok(obj.into())
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

pub(crate) fn js_value_to_text_diff(js: &JsValue) -> Result<TextDiff, JsValue> {
    let arr = js
        .dyn_ref::<Array>()
        .ok_or_else(|| JsValue::from_str("Expected an array"))?;
    let mut builder = DeltaRopeBuilder::new();

    for i in 0..arr.length() {
        let item = arr.get(i);
        let obj = item
            .dyn_ref::<Object>()
            .ok_or_else(|| JsValue::from_str("Expected an object"))?;

        if let Some(retain) = Reflect::get(obj, &JsValue::from_str("retain"))?.as_f64() {
            let len = retain as usize;
            let js_meta = Reflect::get(obj, &JsValue::from_str("attributes"))?;
            let meta = TextMeta::try_from(&js_meta).unwrap_or_default();
            builder = builder.retain(len, meta);
        } else if let Some(insert) = Reflect::get(obj, &JsValue::from_str("insert"))?.as_string() {
            let js_meta = Reflect::get(obj, &JsValue::from_str("attributes"))?;
            let meta = TextMeta::try_from(&js_meta).unwrap_or_default();
            builder = builder.insert(StringSlice::from(insert), meta);
        } else if let Some(delete) = Reflect::get(obj, &JsValue::from_str("delete"))?.as_f64() {
            let len = delete as usize;
            builder = builder.delete(len);
        } else {
            return Err(JsValue::from_str("Invalid delta item"));
        }
    }

    Ok(builder.build())
}

pub(crate) fn js_to_map_delta(js: &JsValue) -> Result<ResolvedMapDelta, JsValue> {
    let obj = js
        .dyn_ref::<Object>()
        .ok_or_else(|| JsValue::from_str("Expected an object"))?;
    let mut delta = ResolvedMapDelta::new();

    let entries = Object::entries(obj);
    for i in 0..entries.length() {
        let entry = entries.get(i);
        let entry_arr = entry
            .dyn_into::<Array>()
            .map_err(|_| JsValue::from_str("Invalid map delta entry"))?;
        let key = entry_arr
            .get(0)
            .as_string()
            .ok_or_else(|| JsValue::from_str("Map delta keys must be strings"))?;
        let value = entry_arr.get(1);

        if value.is_undefined() {
            delta = delta.with_entry(key.into(), ResolvedMapValue::new_unset());
            continue;
        }

        if value.is_object() && !value.is_null() {
            let obj = value
                .dyn_ref::<Object>()
                .ok_or_else(|| JsValue::from_str("Expected an object value"))?;
            if let Ok(kind) = Reflect::get(obj, &JsValue::from_str("kind")) {
                if kind.is_function() {
                    let container = js_to_container(value.clone().unchecked_into())?;
                    delta = delta.with_entry(
                        key.into(),
                        ResolvedMapValue {
                            idlp: IdLp::new(0, 0),
                            value: Some(ValueOrHandler::Handler(container.to_handler())),
                        },
                    );
                    continue;
                }
            }
        }
        let loro_value = js_value_to_loro_value(&value)?;
        delta = delta.with_entry(
            key.into(),
            ResolvedMapValue {
                idlp: IdLp::new(0, 0),
                value: Some(ValueOrHandler::Value(loro_value)),
            },
        );
    }

    Ok(delta)
}

pub(crate) fn js_value_to_list_diff(js: &JsValue) -> Result<ListDiff, JsValue> {
    let arr = js
        .dyn_ref::<Array>()
        .ok_or_else(|| JsValue::from_str("Expected an array"))?;
    let mut builder = DeltaRopeBuilder::new();

    for i in 0..arr.length() {
        let item = arr.get(i);
        let obj = item
            .dyn_ref::<Object>()
            .ok_or_else(|| JsValue::from_str("Expected an object"))?;

        if let Some(retain) = Reflect::get(obj, &JsValue::from_str("retain"))?.as_f64() {
            let len = retain as usize;
            builder = builder.retain(len, ListDeltaMeta::default());
        } else if let Some(delete) = Reflect::get(obj, &JsValue::from_str("delete"))?.as_f64() {
            let len = delete as usize;
            builder = builder.delete(len);
        } else if let Ok(insert) = Reflect::get(obj, &JsValue::from_str("insert")) {
            let insert_arr = insert
                .dyn_ref::<Array>()
                .ok_or_else(|| JsValue::from_str("insert must be an array"))?;
            let mut values = array_vec::ArrayVec::<ValueOrHandler, 8>::new();

            for j in 0..insert_arr.length() {
                let value = insert_arr.get(j);
                if value.is_object() && !value.is_null() {
                    let obj = value
                        .dyn_ref::<Object>()
                        .ok_or_else(|| JsValue::from_str("Insert entries must be objects"))?;
                    if let Ok(kind) = Reflect::get(obj, &JsValue::from_str("kind")) {
                        if kind.is_function() {
                            let container = js_to_container(value.clone().unchecked_into())?;
                            values
                                .push(ValueOrHandler::Handler(container.to_handler()))
                                .map_err(|_| {
                                    JsValue::from_str(
                                        "Insert array exceeds maximum supported length",
                                    )
                                })?;
                            continue;
                        }
                    }
                }
                values
                    .push(ValueOrHandler::Value(js_value_to_loro_value(&value)?))
                    .map_err(|_| {
                        JsValue::from_str("Insert array exceeds maximum supported length")
                    })?
            }

            builder = builder.insert(values, ListDeltaMeta::default());
        } else {
            return Err(JsValue::from_str("Invalid delta item"));
        }
    }

    Ok(builder.build())
}

pub(crate) fn js_value_to_loro_value(js: &JsValue) -> JsResult<LoroValue> {
    if js.is_null() || js.is_undefined() {
        return Ok(LoroValue::Null);
    } else if let Some(b) = js.as_bool() {
        return Ok(LoroValue::Bool(b));
    } else if let Some(n) = js.as_f64() {
        if n.fract() == 0.0 && n >= -(2i64.pow(53) as f64) && n <= 2i64.pow(53) as f64 {
            return Ok(LoroValue::I64(n as i64));
        } else {
            return Ok(LoroValue::Double(n));
        }
    } else if let Some(s) = js.as_string() {
        if let Some(cid) = ContainerID::try_from_loro_value_string(&s) {
            return Ok(LoroValue::Container(cid));
        } else {
            return Ok(LoroValue::String(s.into()));
        }
    } else if js.is_array() {
        let arr = Array::from(js);
        let mut vec = Vec::with_capacity(arr.length() as usize);
        for i in 0..arr.length() {
            vec.push(js_value_to_loro_value(&arr.get(i))?);
        }
        return Ok(LoroValue::List(LoroListValue::from(vec)));
    } else if js.is_object() {
        if js.is_instance_of::<Uint8Array>() {
            let typed_array = Uint8Array::new(js);
            let mut buf = vec![0; typed_array.length() as usize];
            typed_array.copy_to(&mut buf[..]);
            return Ok(LoroValue::Binary(buf.into()));
        }

        if let Some(map) = js.dyn_ref::<Map>() {
            let entries = Array::from(&map.entries().into());
            let mut loro_map = FxHashMap::default();
            for i in 0..entries.length() {
                let entry = entries
                    .get(i)
                    .dyn_into::<Array>()
                    .map_err(|_| JsValue::from_str("Invalid map entry"))?;
                let key = entry
                    .get(0)
                    .as_string()
                    .ok_or_else(|| JsValue::from_str("Map keys must be strings"))?;
                let value = entry.get(1);
                loro_map.insert(key, js_value_to_loro_value(&value)?);
            }
            return Ok(LoroValue::Map(LoroMapValue::from(loro_map)));
        }

        let obj = Object::from(JsValue::from(js));

        if Object::get_own_property_symbols(&obj).length() > 0 {
            return Err(JsValue::from_str(
                "Object keys must be strings; symbol properties are not supported",
            ));
        }
        let mut map = FxHashMap::default();
        let entries = Object::entries(&obj);
        for i in 0..entries.length() {
            let entry = entries
                .get(i)
                .dyn_into::<Array>()
                .map_err(|_| JsValue::from_str("Invalid object entry"))?;
            let key = entry
                .get(0)
                .as_string()
                .ok_or_else(|| JsValue::from_str("Object keys must be strings"))?;
            let value = entry.get(1);
            map.insert(key, js_value_to_loro_value(&value)?);
        }
        return Ok(LoroValue::Map(LoroMapValue::from(map)));
    }

    Err(JsValue::from_str("Unsupported value type"))
}

/// Convert a JavaScript JsonSchema (or string) to Loro's internal JsonSchema
pub(crate) fn js_json_schema_to_loro_json_schema(
    json: JsJsonSchemaOrString,
) -> JsResult<JsonSchema> {
    let js_value: JsValue = json.into();

    if js_value.is_string() {
        let json_str = js_value
            .as_string()
            .ok_or_else(|| JsValue::from_str("JsonSchema must be a string"))?;
        JsonSchema::try_from(json_str.as_str())
            .map_err(|e| JsValue::from_str(&format!("Invalid JSON format: {e}")))
    } else {
        serde_wasm_bindgen::from_value(js_value)
            .map_err(|e| JsValue::from_str(&format!("Failed to parse JsonSchema: {e}")))
    }
}

/// Convert Loro's internal JsonSchema to JavaScript JsonSchema
pub(crate) fn loro_json_schema_to_js_json_schema(
    json_schema: JsonSchema,
) -> JsResult<JsJsonSchema> {
    let s = serde_wasm_bindgen::Serializer::new()
        .serialize_maps_as_objects(true)
        .serialize_missing_as_null(true);
    let value = json_schema
        .serialize(&s)
        .map_err(std::convert::Into::<JsValue>::into)?;
    Ok(value.into())
}

pub(crate) fn text_delta_to_js_value(delta: Vec<TextDelta>) -> Result<JsValue, JsValue> {
    let arr = Array::new();
    let mut iter = delta.into_iter();
    let mut current = iter.next();

    while let Some(mut item) = current {
        let mut next_item = iter.next();

        // Try to merge with next items
        while let Some(next) = next_item.take() {
            match (&mut item, next) {
                (
                    TextDelta::Insert {
                        insert: i1,
                        attributes: a1,
                    },
                    TextDelta::Insert {
                        insert: i2,
                        attributes: a2,
                    },
                ) if a1 == &a2 => {
                    i1.push_str(&i2);
                    // next_item is consumed, try next
                    next_item = iter.next();
                }
                (
                    TextDelta::Retain {
                        retain: r1,
                        attributes: a1,
                    },
                    TextDelta::Retain {
                        retain: r2,
                        attributes: a2,
                    },
                ) if a1 == &a2 => {
                    *r1 += r2;
                    next_item = iter.next();
                }
                (TextDelta::Delete { delete: d1 }, TextDelta::Delete { delete: d2 }) => {
                    *d1 += d2;
                    next_item = iter.next();
                }
                (_, next) => {
                    // Cannot merge, break inner loop and save next item for next iteration
                    next_item = Some(next);
                    break;
                }
            }
        }

        // Convert merged item to JS
        let obj = Object::new();
        match item {
            TextDelta::Insert { insert, attributes } => {
                Reflect::set(&obj, &"insert".into(), &insert.into())?;
                if let Some(attributes) = attributes {
                    set_style_attributes(&obj, attributes)?;
                }
            }
            TextDelta::Retain { retain, attributes } => {
                Reflect::set(&obj, &"retain".into(), &retain.into())?;
                if let Some(attributes) = attributes {
                    set_style_attributes(&obj, attributes)?;
                }
            }
            TextDelta::Delete { delete } => {
                Reflect::set(&obj, &"delete".into(), &delete.into())?;
            }
        }
        arr.push(&obj);

        current = next_item;
    }
    Ok(arr.into())
}

fn set_style_attributes(
    obj: &Object,
    attributes: FxHashMap<String, LoroValue>,
) -> Result<(), JsValue> {
    if !attributes.is_empty() {
        let attrs = Object::new();
        let mut is_empty = true;
        for (k, v) in attributes {
            if v.is_null() {
                // We should ignore attribute with null value, since it should be treated as deleted
                continue;
            }

            is_empty = false;
            Reflect::set(&attrs, &k.into(), &convert(v)?)?;
        }

        if !is_empty {
            Reflect::set(obj, &"attributes".into(), &attrs)?;
        }
    }
    Ok(())
}
