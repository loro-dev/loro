use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use loro_core::{
    container::{
        manager::LockContainer,
        text::text_container::{self, TextContainer},
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
        let get_or_create_root_text = loro.get_or_create_root_text(name);
        let text_container = get_or_create_root_text.lock_text();
        Ok(Text {
            id: text_container.id().clone(),
            loro: Rc::downgrade(&self.loro),
        })
    }

    pub fn get_map_container(&mut self, name: &str) -> Result<Map, JsValue> {
        let mut loro = self.loro.borrow_mut();
        let get_or_create_root_map = loro.get_or_create_root_map(name);
        let map = get_or_create_root_map.lock_map();
        Ok(Map {
            id: map.id().clone(),
            loro: Rc::downgrade(&self.loro),
        })
    }
}

#[wasm_bindgen]
impl Map {
    pub fn set(&mut self, key: String, value: JsValue) {
        let loro = self.loro.upgrade().unwrap();
        let mut loro = loro.borrow_mut();
        let get_map_container_mut = loro.get_container(&self.id).unwrap();
        let mut map = get_map_container_mut.lock_map();
        map.insert(key.into(), InsertValue::try_from_js(value).unwrap())
    }

    pub fn delete(&mut self, key: String) {
        let loro = self.loro.upgrade().unwrap();
        let mut loro = loro.borrow_mut();
        let mut map = loro.get_container(&self.id).unwrap();
        let mut map = map.lock_map();
        map.delete(key.into())
    }

    pub fn get_value(&mut self) -> JsValue {
        let loro = self.loro.upgrade().unwrap();
        let mut loro = loro.borrow_mut();
        let mut map = loro.get_container(&self.id).unwrap();
        let mut map = map.lock_map();
        map.get_value().clone().into()
    }
}

#[wasm_bindgen]
impl Text {
    pub fn insert(&mut self, index: usize, text: &str) {
        let loro = self.loro.upgrade().unwrap();
        let mut loro = loro.borrow_mut();
        let mut text_container = loro.get_container(&self.id).unwrap();
        let mut text_container = text_container.lock_text();
        text_container.insert(index, text);
    }

    pub fn delete(&mut self, index: usize, len: usize) {
        let loro = self.loro.upgrade().unwrap();
        let mut loro = loro.borrow_mut();
        let get_container = loro.get_container(&self.id).unwrap();
        let mut text_container = get_container.lock_text();
        text_container.delete(index, len);
    }

    pub fn get_value(&mut self) -> String {
        let loro = self.loro.upgrade().unwrap();
        let mut loro = loro.borrow_mut();
        let get_container = loro.get_container(&self.id).unwrap();
        let mut text_container = get_container.lock_text();
        text_container.get_value().as_string().unwrap().to_string()
    }
}

impl Default for Loro {
    fn default() -> Self {
        Self::new()
    }
}
