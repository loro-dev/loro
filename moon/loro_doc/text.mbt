///|
pub struct LoroText {
  doc : LoroDoc
  cid : @loro_codec.ContainerID
}

///|
pub fn LoroText::asContainer(self : LoroText) -> Container {
  Container::new(self.doc, self.cid)
}

///|
pub fn LoroText::kind(self : LoroText) -> @loro_codec.ContainerType {
  @loro_codec.ContainerType::text()
}

///|
pub fn LoroText::id(self : LoroText) -> @loro_codec.ContainerID {
  self.cid
}

///|
pub fn LoroText::isAttached(self : LoroText) -> Bool {
  self.doc.has_container(self.cid)
}

///|
pub fn LoroText::isDeleted(self : LoroText) -> Bool {
  false
}

///|
pub fn LoroText::getShallowValue(self : LoroText) -> Json {
  self.doc.container_shallow_json(self.cid)
}

///|
pub fn LoroText::toJSON(self : LoroText) -> Json {
  self.doc.container_deep_json(self.cid)
}

///|
pub fn LoroText::length(self : LoroText) -> Int {
  0
}

///|
pub fn LoroText::toString(self : LoroText) -> String {
  ""
}

///|
pub fn LoroText::insert(self : LoroText, _index : Int, _text : String) -> Unit raise LoroError {
  raise LoroError("Text is not implemented yet")
}

///|
pub fn LoroText::delete(self : LoroText, _index : Int, _len : Int) -> Unit raise LoroError {
  raise LoroError("Text is not implemented yet")
}

