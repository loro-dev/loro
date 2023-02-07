#![deny(clippy::all)]

use loro_internal::{LoroCore, Text};

#[macro_use]
extern crate napi_derive;

#[napi]
#[derive(Default)]
pub struct Loro(LoroCore);

#[napi]
impl Loro {
  #[napi(constructor)]
  pub fn new() -> Self {
    Self(LoroCore::default())
  }

  #[napi]
  pub fn get_text(&mut self, id: String) -> LoroText {
    LoroText(self.0.get_text(id))
  }
}

#[napi]
pub struct LoroText(Text);

#[napi]
impl LoroText {
  #[napi]
  pub fn insert(&mut self, loro: &Loro, pos: u32, text: String) {
    self.0.insert(&loro.0, pos as usize, &text).unwrap()
  }

  #[napi]
  pub fn value(&self) -> String {
    self.0.get_value().as_string().unwrap().to_string()
  }
}
