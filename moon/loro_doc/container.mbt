///|
pub struct Container {
  doc : LoroDoc
  cid : @loro_codec.ContainerID
}

///|
pub fn Container::new(doc : LoroDoc, cid : @loro_codec.ContainerID) -> Container {
  { doc, cid }
}

///| Create a detached container template of the given kind.
pub fn Container::detached(kind : @loro_codec.ContainerType) -> Container {
  let doc = LoroDoc::new()
  let cid = @loro_codec.ContainerID::normal(0UL, 0, kind)
  Container::new(doc, cid)
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

///|
pub fn Container::asMap(self : Container) -> LoroMap raise LoroError {
  if self.kind() != @loro_codec.ContainerType::map() {
    raise LoroError("container.asMap: kind mismatch")
  }
  { doc: self.doc, cid: self.cid }
}

///|
pub fn Container::asList(self : Container) -> LoroList raise LoroError {
  if self.kind() != @loro_codec.ContainerType::list() {
    raise LoroError("container.asList: kind mismatch")
  }
  { doc: self.doc, cid: self.cid }
}

///|
pub fn Container::asMovableList(self : Container) -> LoroMovableList raise LoroError {
  if self.kind() != @loro_codec.ContainerType::movable_list() {
    raise LoroError("container.asMovableList: kind mismatch")
  }
  { doc: self.doc, cid: self.cid }
}

///|
pub fn Container::asText(self : Container) -> LoroText raise LoroError {
  if self.kind() != @loro_codec.ContainerType::text() {
    raise LoroError("container.asText: kind mismatch")
  }
  { doc: self.doc, cid: self.cid }
}

///|
pub fn Container::asTree(self : Container) -> LoroTree raise LoroError {
  if self.kind() != @loro_codec.ContainerType::tree() {
    raise LoroError("container.asTree: kind mismatch")
  }
  { doc: self.doc, cid: self.cid }
}
