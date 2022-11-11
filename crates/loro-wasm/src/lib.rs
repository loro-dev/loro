use std::{
    cell::RefCell,
    ops::Deref,
    rc::{Rc, Weak},
};

use loro_core::{
    container::{
        registry::{ContainerWrapper, LockContainer},
        Container, ContainerID,
    },
    InsertValue, LoroCore,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Loro {
    loro: Rc<RefCell<LoroCore>>,
}

#[wasm_bindgen]
pub struct Text {
    loro: Weak<RefCell<LoroCore>>,
    id: ContainerID,
}

#[wasm_bindgen]
pub struct Map {
    loro: Weak<RefCell<LoroCore>>,
    id: ContainerID,
}

#[wasm_bindgen]
impl Loro {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Loro {
            // TODO: expose the configuration
            loro: Rc::new(RefCell::new(LoroCore::default())),
        }
    }

    pub fn get_text_container(&mut self, name: &str) -> Result<Text, JsValue> {
        let mut loro = self.loro.borrow_mut();
        let text_container = loro.get_text(name);
        Ok(Text {
            id: text_container.id(),
            loro: Rc::downgrade(&self.loro),
        })
    }

    pub fn get_map_container(&mut self, name: &str) -> Result<Map, JsValue> {
        let mut loro = self.loro.borrow_mut();
        let map = loro.get_map(name);
        Ok(Map {
            id: map.id(),
            loro: Rc::downgrade(&self.loro),
        })
    }
}

#[wasm_bindgen]
impl Map {
    pub fn set(&mut self, key: String, value: JsValue) {
        let loro = self.loro.upgrade().unwrap();
        let loro = loro.borrow_mut();
        let get_map_container_mut = loro.get_container(&self.id).unwrap();
        let mut map = get_map_container_mut.lock_map();
        map.insert(
            loro.deref(),
            key.into(),
            InsertValue::try_from_js(value).unwrap(),
        )
    }

    pub fn delete(&mut self, key: String) {
        let loro = self.loro.upgrade().unwrap();
        let loro = loro.borrow_mut();
        let map = loro.get_container(&self.id).unwrap();
        let mut map = map.lock_map();
        map.delete(loro.deref(), key.into())
    }

    pub fn get_value(&mut self) -> JsValue {
        let loro = self.loro.upgrade().unwrap();
        let loro = loro.borrow_mut();
        let map = loro.get_container(&self.id).unwrap();
        let map = map.lock_map();
        map.get_value().into()
    }
}

#[wasm_bindgen]
impl Text {
    pub fn insert(&mut self, index: usize, text: &str) {
        let loro = self.loro.upgrade().unwrap();
        let loro = loro.borrow_mut();
        let text_container = loro.get_container(&self.id).unwrap();
        let mut text_container = text_container.lock_text();
        text_container.insert(loro.deref(), index, text);
    }

    pub fn delete(&mut self, index: usize, len: usize) {
        let loro = self.loro.upgrade().unwrap();
        let loro = loro.borrow_mut();
        let get_container = loro.get_container(&self.id).unwrap();
        let mut text_container = get_container.lock_text();
        text_container.delete(loro.deref(), index, len);
    }

    pub fn get_value(&mut self) -> String {
        let loro = self.loro.upgrade().unwrap();
        let loro = loro.borrow_mut();
        let get_container = loro.get_container(&self.id).unwrap();
        let text_container = get_container.lock_text();
        text_container.get_value().as_string().unwrap().to_string()
    }
}

impl Default for Loro {
    fn default() -> Self {
        Self::new()
    }
}
