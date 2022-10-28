use loro_core::{
    configure::Configure,
    container::{manager::ContainerRef, text::text_container::TextContainer as LoroTextContainer},
    LoroCore,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Loro {
    loro: LoroCore,
}

#[wasm_bindgen]
pub struct TextContainer {
    inner: ContainerRef<'static, LoroTextContainer>,
}

impl Loro {
    pub fn new() -> Self {
        Loro {
            // TODO: expose the configuration
            loro: LoroCore::default(),
        }
    }

    pub fn get_text_container(&mut self, name: &str) -> TextContainer {
        TextContainer {
            inner: self.loro.get_or_create_text_container_mut(name),
        }
    }
}

impl Default for Loro {
    fn default() -> Self {
        Self::new()
    }
}
