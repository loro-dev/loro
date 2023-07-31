use js_sys::{Object, Reflect};
use wasm_bindgen::JsValue;

use crate::{LoroList, LoroMap, LoroText, PrelimList, PrelimMap, PrelimText};
use wasm_bindgen::convert::FromWasmAbi;

/// Convert a `JsValue` to `T` by constructor's name.
///
/// more details can be found in https://github.com/rustwasm/wasm-bindgen/issues/2231#issuecomment-656293288
pub(crate) fn js_to_any<T: FromWasmAbi<Abi = u32>>(
    js: JsValue,
    struct_name: &str,
) -> Result<T, JsValue> {
    if !js.is_object() {
        return Err(JsValue::from_str(
            format!("Value supplied as {} is not an object", struct_name).as_str(),
        ));
    }
    let ctor_name = Object::get_prototype_of(&js).constructor().name();
    if ctor_name == struct_name {
        let ptr = Reflect::get(&js, &JsValue::from_str("ptr"))?;
        let ptr_u32: u32 = ptr.as_f64().ok_or(JsValue::NULL)? as u32;
        let obj = unsafe { T::from_abi(ptr_u32) };
        Ok(obj)
    } else {
        return Err(JsValue::from_str(
            format!(
                "Value ctor_name is {} but the required struct name is {}",
                ctor_name, struct_name
            )
            .as_str(),
        ));
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
