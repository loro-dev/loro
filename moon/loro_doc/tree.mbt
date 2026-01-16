///|
pub struct LoroTree {
  doc : LoroDoc
  cid : @loro_codec.ContainerID
}

///|
pub fn LoroTree::asContainer(self : LoroTree) -> Container {
  Container::new(self.doc, self.cid)
}

///|
pub fn LoroTree::kind(self : LoroTree) -> @loro_codec.ContainerType {
  @loro_codec.ContainerType::tree()
}

///|
pub fn LoroTree::id(self : LoroTree) -> @loro_codec.ContainerID {
  self.cid
}

///|
pub fn LoroTree::isAttached(self : LoroTree) -> Bool {
  self.doc.has_container(self.cid)
}

///|
pub fn LoroTree::isDeleted(self : LoroTree) -> Bool {
  false
}

///|
pub fn LoroTree::getShallowValue(self : LoroTree) -> Json {
  self.doc.container_shallow_json(self.cid)
}

///|
pub fn LoroTree::toJSON(self : LoroTree) -> Json {
  self.doc.container_deep_json(self.cid)
}

