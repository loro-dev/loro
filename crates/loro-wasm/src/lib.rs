use std::{
    cell::RefCell,
    ops::Deref,
    rc::{Rc, Weak},
};

use loro_core::{container::ContainerID, LoroCore};
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
        let mut loro = loro.borrow_mut();
        let mut map = loro.get_map(&self.id);
        map.insert(loro.deref(), &key, value)
    }

    pub fn delete(&mut self, key: String) {
        let loro = self.loro.upgrade().unwrap();
        let mut loro = loro.borrow_mut();
        let mut map = loro.get_map(&self.id);
        map.delete(loro.deref(), &key)
    }

    pub fn get_value(&mut self) -> JsValue {
        let loro = self.loro.upgrade().unwrap();
        let mut loro = loro.borrow_mut();
        let map = loro.get_map(&self.id);
        map.get_value().into()
    }
}

#[wasm_bindgen]
impl Text {
    pub fn insert(&mut self, index: usize, text: &str) {
        let loro = self.loro.upgrade().unwrap();
        let mut loro = loro.borrow_mut();
        let mut text_container = loro.get_text(&self.id);
        text_container.insert(loro.deref(), index, text);
    }

    pub fn delete(&mut self, index: usize, len: usize) {
        let loro = self.loro.upgrade().unwrap();
        let mut loro = loro.borrow_mut();
        let mut text_container = loro.get_text(&self.id);
        text_container.delete(loro.deref(), index, len);
    }

    pub fn get_value(&mut self) -> String {
        let loro = self.loro.upgrade().unwrap();
        let mut loro = loro.borrow_mut();
        let text_container = loro.get_text(&self.id);
        text_container.get_value().as_string().unwrap().to_string()
    }
}

impl Default for Loro {
    fn default() -> Self {
        Self::new()
    }
}
