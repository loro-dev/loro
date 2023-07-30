use loro_internal::FxHashMap;
use wasm_bindgen::prelude::*;

use crate::JsResult;

pub(crate) enum PrelimType {
    Text(PrelimText),
    Map(PrelimMap),
    List(PrelimList),
}

#[wasm_bindgen]
pub struct PrelimText(String);

#[wasm_bindgen]
impl PrelimText {
    #[wasm_bindgen(constructor)]
    pub fn new(text: Option<String>) -> Self {
        Self(text.unwrap_or_default())
    }

    pub fn insert(&mut self, index: usize, text: &str) {
        self.0.insert_str(index, text);
    }

    pub fn delete(&mut self, index: usize, len: usize) {
        self.0.drain(index..index + len);
    }

    #[wasm_bindgen(js_name = "value", method, getter)]
    pub fn get_value(&self) -> String {
        self.0.clone()
    }
}

#[wasm_bindgen]
pub struct PrelimList(Vec<JsValue>);

#[wasm_bindgen]
impl PrelimList {
    #[wasm_bindgen(constructor)]
    pub fn new(list: Option<Vec<JsValue>>) -> Self {
        Self(list.unwrap_or_default())
    }

    pub fn insert(&mut self, index: usize, value: JsValue) {
        self.0.insert(index, value);
    }

    pub fn delete(&mut self, index: usize, len: usize) {
        self.0.drain(index..index + len);
    }

    #[wasm_bindgen(js_name = "value", method, getter)]
    pub fn get_value(&self) -> Vec<JsValue> {
        self.0.clone()
    }
}

#[wasm_bindgen]
pub struct PrelimMap(FxHashMap<String, JsValue>);

#[wasm_bindgen]
impl PrelimMap {
    #[wasm_bindgen(constructor)]
    pub fn new(obj: Option<js_sys::Object>) -> Self {
        let map = if let Some(object) = obj {
            let mut map = FxHashMap::default();
            let entries = js_sys::Object::entries(&object);
            for tuple in entries.iter() {
                let tuple = js_sys::Array::from(&tuple);
                let key = tuple.get(0).as_string().unwrap();
                let value = tuple.get(1);
                map.insert(key, value);
            }
            map
        } else {
            FxHashMap::default()
        };
        Self(map)
    }

    #[wasm_bindgen(js_name = set)]
    pub fn insert(&mut self, key: &str, value: JsValue) {
        self.0.insert(key.to_string(), value);
    }

    pub fn delete(&mut self, key: &str) {
        self.0.remove(key);
    }

    pub fn get(&self, key: &str) -> JsResult<JsValue> {
        if let Some(v) = self.0.get(key).cloned() {
            Ok(v)
        } else {
            Err(JsValue::from_str("Key not found"))
        }
    }

    // TODO: entries iterator
    #[wasm_bindgen(js_name = "value", method, getter)]
    pub fn get_value(&self) -> js_sys::Object {
        let object = js_sys::Object::new();
        for (key, value) in self.0.iter() {
            js_sys::Reflect::set(&object, &key.into(), value).unwrap();
        }
        object
    }
}
