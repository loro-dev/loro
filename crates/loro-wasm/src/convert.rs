use js_sys::{Object, Reflect};
use wasm_bindgen::JsValue;

use crate::{prelim::PrelimType, LoroList, LoroMap, LoroText, PrelimList, PrelimMap, PrelimText};
use wasm_bindgen::convert::FromWasmAbi;
pub(crate) fn js_to_any<T: FromWasmAbi<Abi = u32>>(
    js: JsValue,
    struct_name: &str,
) -> Result<T, JsValue> {
    let ctor_name = Object::get_prototype_of(&js).constructor().name();
    if ctor_name == struct_name {
        let ptr = Reflect::get(&js, &JsValue::from_str("ptr"))?;
        let ptr_u32: u32 = ptr.as_f64().ok_or(JsValue::NULL)? as u32;
        let obj = unsafe { T::from_abi(ptr_u32) };
        Ok(obj)
    } else {
        Err(JsValue::NULL)
    }
}

impl TryFrom<JsValue> for PrelimText {
    type Error = JsValue;

    fn try_from(value: JsValue) -> Result<Self, Self::Error> {
        js_to_any(value, "PrelimText")
    }
}

impl TryFrom<JsValue> for PrelimList {
    type Error = JsValue;

    fn try_from(value: JsValue) -> Result<Self, Self::Error> {
        js_to_any(value, "PrelimList")
    }
}

impl TryFrom<JsValue> for PrelimMap {
    type Error = JsValue;

    fn try_from(value: JsValue) -> Result<Self, Self::Error> {
        js_to_any(value, "PrelimMap")
    }
}

impl TryFrom<JsValue> for LoroText {
    type Error = JsValue;

    fn try_from(value: JsValue) -> Result<Self, Self::Error> {
        js_to_any(value, "LoroText")
    }
}

impl TryFrom<JsValue> for LoroList {
    type Error = JsValue;

    fn try_from(value: JsValue) -> Result<Self, Self::Error> {
        js_to_any(value, "LoroList")
    }
}

impl TryFrom<JsValue> for LoroMap {
    type Error = JsValue;

    fn try_from(value: JsValue) -> Result<Self, Self::Error> {
        js_to_any(value, "LoroMap")
    }
}

pub(crate) fn js_try_to_prelim(value: &JsValue) -> Option<PrelimType> {
    let ctor_name = Object::get_prototype_of(value).constructor().name();
    match ctor_name.as_string().unwrap().as_ref() {
        "PrelimText" => Some(PrelimText::try_from(value.clone()).unwrap().into()),
        "PrelimList" => Some(PrelimList::try_from(value.clone()).unwrap().into()),
        "PrelimMap" => Some(PrelimMap::try_from(value.clone()).unwrap().into()),
        _ => None,
    }
}
