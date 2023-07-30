#![deny(clippy::all)]

use loro_internal::{LoroDoc, TextHandler};

#[macro_use]
extern crate napi_derive;

#[napi]
#[derive(Default)]
pub struct Loro(LoroDoc);

#[napi]
impl Loro {
  #[napi(constructor)]
  pub fn new() -> Self {
    Self(LoroDoc::default())
  }

  #[napi]
  pub fn get_text(&mut self, id: String) -> LoroText {
    LoroText(self.0.get_text(id))
  }
}

#[napi]
pub struct LoroText(TextHandler);

#[napi]
impl LoroText {
  #[napi]
  pub fn insert(&mut self, loro: &Loro, pos: u32, text: String) {
    let mut txn = loro.0.txn().unwrap();
    self.0.insert(&mut txn, pos as usize, &text).unwrap()
  }

  #[napi]
  pub fn value(&self) -> String {
    self.0.get_value().as_string().unwrap().to_string()
  }
}
