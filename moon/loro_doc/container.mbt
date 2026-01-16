///|
pub struct Container {
  doc : LoroDoc
  cid : @loro_codec.ContainerID
}

///|
pub fn Container::new(doc : LoroDoc, cid : @loro_codec.ContainerID) -> Container {
  { doc, cid }
}

///|
pub fn Container::doc(self : Container) -> LoroDoc {
  self.doc
}

///|
pub fn Container::id(self : Container) -> @loro_codec.ContainerID {
  self.cid
}

///|
pub fn Container::kind(self : Container) -> @loro_codec.ContainerType {
  match self.cid {
    @loro_codec.ContainerID::Root(_name, kind) => kind
    @loro_codec.ContainerID::Normal(_peer, _counter, kind) => kind
  }
}

///|
pub fn Container::isAttached(self : Container) -> Bool {
  self.doc.has_container(self.cid)
}

///|
pub fn Container::isDeleted(self : Container) -> Bool {
  // TODO(runtime): implement reachability-based deletion.
  false
}

///|
pub fn Container::getShallowValue(self : Container) -> Json {
  self.doc.container_shallow_json(self.cid)
}

///|
pub fn Container::toJSON(self : Container) -> Json {
  self.doc.container_deep_json(self.cid)
}

