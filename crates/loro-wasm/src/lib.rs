use std::ops::{Deref, DerefMut};

use loro_core::{
    container::registry::ContainerWrapper, context::Context, ContainerType, List, LoroCore, Map,
    Text,
};
use wasm_bindgen::prelude::*;

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
    pub fn get_text(&mut self, name: &str) -> Result<LoroText, JsValue> {
        let text = self.0.get_text(name);
        Ok(LoroText(text))
    }

    #[wasm_bindgen(js_name = "getMap")]
    pub fn get_map(&mut self, name: &str) -> Result<LoroMap, JsValue> {
        let map = self.0.get_map(name);
        Ok(LoroMap(map))
    }
}

#[wasm_bindgen]
pub struct LoroText(Text);

#[wasm_bindgen]
impl LoroText {
    pub fn insert(&mut self, ctx: &Loro, index: usize, content: &str) {
        self.0.insert(ctx.deref(), index, content);
    }

    pub fn delete(&mut self, ctx: &Loro, index: usize, len: usize) {
        self.0.delete(ctx.deref(), index, len);
    }

    #[wasm_bindgen(js_name = "value", method, getter)]
    pub fn get_value(&mut self) -> String {
        self.0.get_value().as_string().unwrap().to_string()
    }
}

#[wasm_bindgen]
pub struct LoroMap(Map);

#[wasm_bindgen]
impl LoroMap {
    #[wasm_bindgen(js_name = "set")]
    pub fn insert(&mut self, ctx: &Loro, key: &str, value: JsValue) {
        self.0.insert(ctx.deref(), key, value);
    }

    pub fn delete(&mut self, ctx: &Loro, key: &str) {
        self.0.delete(ctx.deref(), key);
    }

    #[wasm_bindgen(js_name = "value", method, getter)]
    pub fn get_value(&mut self) -> JsValue {
        self.0.get_value().into()
    }

    #[wasm_bindgen(js_name = "getValueDeep")]
    pub fn get_value_deep(&mut self, ctx: &Loro) -> JsValue {
        self.0.get_value_deep(ctx.deref()).into()
    }

    #[wasm_bindgen(js_name = "getText")]
    pub fn get_text(&mut self, ctx: &mut Loro, key: &str) -> LoroText {
        let id = self.0.insert_obj(&ctx.0, key, ContainerType::Text);
        let text = ctx.deref().get_container(&id).unwrap();
        LoroText(text.into())
    }

    #[wasm_bindgen(js_name = "getMap")]
    pub fn get_map(&mut self, ctx: &mut Loro, key: &str) -> LoroMap {
        let id = self.0.insert_obj(ctx.deref_mut(), key, ContainerType::Map);
        let map = ctx.deref().get_container(&id).unwrap();
        LoroMap(map.into())
    }

    #[wasm_bindgen(js_name = "getList")]
    pub fn get_list(&mut self, ctx: &mut Loro, key: &str) -> LoroList {
        let id = self.0.insert_obj(ctx.deref_mut(), key, ContainerType::List);
        let list = ctx.deref().get_container(&id).unwrap();
        LoroList(list.into())
    }
}

#[wasm_bindgen]
pub struct LoroList(List);

#[wasm_bindgen]
impl LoroList {
    #[wasm_bindgen(js_name = "set")]
    pub fn insert(&mut self, ctx: &Loro, index: usize, value: JsValue) {
        self.0.insert(ctx.deref(), index, value);
    }

    pub fn delete(&mut self, ctx: &Loro, index: usize, len: usize) {
        self.0.delete(ctx.deref(), index, len);
    }

    #[wasm_bindgen(js_name = "value", method, getter)]
    pub fn get_value(&mut self) -> JsValue {
        self.0.get_value().into()
    }

    #[wasm_bindgen(js_name = "getValueDeep")]
    pub fn get_value_deep(&mut self, ctx: &Loro) -> JsValue {
        self.0.get_value_deep(ctx.deref()).into()
    }

    #[wasm_bindgen(js_name = "getText")]
    pub fn get_text(&mut self, ctx: &mut Loro, index: usize) -> LoroText {
        let id = self
            .0
            .insert_obj(ctx.deref_mut(), index, ContainerType::Text);
        let text = ctx.deref().get_container(&id).unwrap();
        LoroText(text.into())
    }

    #[wasm_bindgen(js_name = "getMap")]
    pub fn get_map(&mut self, ctx: &mut Loro, index: usize) -> LoroMap {
        let id = self
            .0
            .insert_obj(ctx.deref_mut(), index, ContainerType::Map);
        let map = ctx.deref().get_container(&id).unwrap();
        LoroMap(map.into())
    }

    #[wasm_bindgen(js_name = "getList")]
    pub fn get_list(&mut self, ctx: &mut Loro, index: usize) -> LoroList {
        let id = self
            .0
            .insert_obj(ctx.deref_mut(), index, ContainerType::List);
        let list = ctx.deref().get_container(&id).unwrap();
        LoroList(list.into())
    }
}

impl Default for Loro {
    fn default() -> Self {
        Self::new()
    }
}
