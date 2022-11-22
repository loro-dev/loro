use loro_core::{
    container::registry::ContainerWrapper, context::Context, ContainerType, List, LoroCore, Map,
    Text,
};
use std::ops::{Deref, DerefMut};
use wasm_bindgen::prelude::*;
mod prelim;
pub use prelim::{PrelimList, PrelimMap, PrelimText};

use crate::convert::js_try_to_prelim;
mod convert;

#[wasm_bindgen(js_name = setPanicHook)]
pub fn set_panic_hook() {
    // When the `console_error_panic_hook` feature is enabled, we can call the
    // `set_panic_hook` function at least once during initialization, and then
    // we will get better error messages if our code ever panics.
    //
    // For more details see
    // https://github.com/rustwasm/console_error_panic_hook#readme
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

type JsResult<T> = Result<T, JsValue>;

#[wasm_bindgen]
pub struct Loro(LoroCore);

impl Deref for Loro {
    type Target = LoroCore;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Loro {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[wasm_bindgen]
impl Loro {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self(LoroCore::default())
    }

    #[wasm_bindgen(js_name = "getText")]
    pub fn get_text(&mut self, name: &str) -> JsResult<LoroText> {
        let text = self.0.get_text(name);
        Ok(LoroText(text))
    }

    #[wasm_bindgen(js_name = "getMap")]
    pub fn get_map(&mut self, name: &str) -> JsResult<LoroMap> {
        let map = self.0.get_map(name);
        Ok(LoroMap(map))
    }

    #[wasm_bindgen(js_name = "getList")]
    pub fn get_list(&mut self, name: &str) -> JsResult<LoroList> {
        let list = self.0.get_list(name);
        Ok(LoroList(list))
    }
}

#[wasm_bindgen]
pub struct LoroText(Text);

#[wasm_bindgen]
impl LoroText {
    pub fn insert(&mut self, ctx: &Loro, index: usize, content: &str) -> JsResult<()> {
        self.0.insert(ctx.deref(), index, content)?;
        Ok(())
    }

    pub fn delete(&mut self, ctx: &Loro, index: usize, len: usize) -> JsResult<()> {
        self.0.delete(ctx.deref(), index, len)?;
        Ok(())
    }

    #[wasm_bindgen(js_name = "value", method, getter)]
    pub fn get_value(&self) -> String {
        self.0.get_value().as_string().unwrap().to_string()
    }
}

#[wasm_bindgen]
pub struct LoroMap(Map);

#[wasm_bindgen]
impl LoroMap {
    #[wasm_bindgen(js_name = "set")]
    pub fn insert(&mut self, ctx: &Loro, key: &str, value: JsValue) -> JsResult<()> {
        if let Some(v) = js_try_to_prelim(&value) {
            self.0.insert(ctx.deref(), key, v)?;
        } else {
            self.0.insert(ctx.deref(), key, value)?;
        };
        Ok(())
    }

    pub fn delete(&mut self, ctx: &Loro, key: &str) -> JsResult<()> {
        self.0.delete(ctx.deref(), key)?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> JsValue {
        self.0.get(key).into()
    }

    #[wasm_bindgen(js_name = "value", method, getter)]
    pub fn get_value(&self) -> JsValue {
        // TODO: if unresolved, return a container ID
        self.0.get_value().into()
    }

    #[wasm_bindgen(js_name = "getValueDeep")]
    pub fn get_value_deep(&self, ctx: &Loro) -> JsValue {
        self.0.get_value_deep(ctx.deref()).into()
    }

    #[wasm_bindgen(js_name = "setContainer")]
    pub fn set_container(
        &mut self,
        ctx: &mut Loro,
        key: &str,
        container: &str,
    ) -> JsResult<JsValue> {
        let _type = match container {
            "text" => ContainerType::Text,
            "map" => ContainerType::Map,
            "list" => ContainerType::List,
            _ => return Err(JsValue::from_str("Invalid container type")),
        };
        let id = self.0.insert(&ctx.0, key, _type)?.unwrap();
        let instance = ctx.deref().get_container(&id).unwrap();
        let container = match _type {
            ContainerType::Text => {
                LoroText(Text::from_instance(instance, ctx.deref().client_id())).into()
            }
            ContainerType::Map => {
                LoroMap(Map::from_instance(instance, ctx.deref().client_id())).into()
            }
            ContainerType::List => {
                LoroList(List::from_instance(instance, ctx.deref().client_id())).into()
            }
        };
        Ok(container)
    }
}

#[wasm_bindgen]
pub struct LoroList(List);

#[wasm_bindgen]
impl LoroList {
    pub fn insert(&mut self, ctx: &Loro, index: usize, value: JsValue) -> JsResult<()> {
        if let Some(v) = js_try_to_prelim(&value) {
            self.0.insert(ctx.deref(), index, v)?;
        } else {
            self.0.insert(ctx.deref(), index, value)?;
        };
        Ok(())
    }

    pub fn delete(&mut self, ctx: &Loro, index: usize, len: usize) -> JsResult<()> {
        self.0.delete(ctx.deref(), index, len)?;
        Ok(())
    }

    pub fn get(&self, index: usize) -> JsValue {
        self.0.get(index).into()
    }

    #[wasm_bindgen(js_name = "value", method, getter)]
    pub fn get_value(&mut self) -> JsValue {
        self.0.get_value().into()
    }

    #[wasm_bindgen(js_name = "getValueDeep")]
    pub fn get_value_deep(&self, ctx: &Loro) -> JsValue {
        let value = self.0.get_value_deep(ctx.deref());
        value.into()
    }

    #[wasm_bindgen(js_name = "setContainer")]
    pub fn set_container(
        &mut self,
        ctx: &mut Loro,
        pos: usize,
        container: &str,
    ) -> JsResult<JsValue> {
        let _type = match container {
            "text" => ContainerType::Text,
            "map" => ContainerType::Map,
            "list" => ContainerType::List,
            _ => return Err(JsValue::from_str("Invalid container type")),
        };
        let id = self.0.insert(&ctx.0, pos, _type)?.unwrap();
        let instance = ctx.deref().get_container(&id).unwrap();
        let container = match _type {
            ContainerType::Text => {
                LoroText(Text::from_instance(instance, ctx.deref().client_id())).into()
            }
            ContainerType::Map => {
                LoroMap(Map::from_instance(instance, ctx.deref().client_id())).into()
            }
            ContainerType::List => {
                LoroList(List::from_instance(instance, ctx.deref().client_id())).into()
            }
        };
        Ok(container)
    }
}

impl Default for Loro {
    fn default() -> Self {
        Self::new()
    }
}
