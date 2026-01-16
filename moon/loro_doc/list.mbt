///|
pub struct LoroList {
  doc : LoroDoc
  cid : @loro_codec.ContainerID
}

///|
pub fn LoroList::asContainer(self : LoroList) -> Container {
  Container::new(self.doc, self.cid)
}

///|
pub fn LoroList::kind(self : LoroList) -> @loro_codec.ContainerType {
  @loro_codec.ContainerType::list()
}

///|
pub fn LoroList::id(self : LoroList) -> @loro_codec.ContainerID {
  self.cid
}

///|
pub fn LoroList::isAttached(self : LoroList) -> Bool {
  self.doc.has_container(self.cid)
}

///|
pub fn LoroList::isDeleted(self : LoroList) -> Bool {
  false
}

///|
pub fn LoroList::getShallowValue(self : LoroList) -> Json {
  self.doc.container_shallow_json(self.cid)
}

///|
pub fn LoroList::toJSON(self : LoroList) -> Json {
  self.doc.container_deep_json(self.cid)
}

///|
pub fn LoroList::length(self : LoroList) -> Int {
  0
}

///|
pub fn LoroList::get(self : LoroList, _index : Int) -> Value? {
  None
}

///|
pub fn LoroList::insert(
  self : LoroList,
  _index : Int,
  _value : Value,
) -> Unit raise LoroError {
  raise LoroError("List is not implemented yet")
}

///|
pub fn LoroList::delete(self : LoroList, _index : Int, _len : Int) -> Unit raise LoroError {
  raise LoroError("List is not implemented yet")
}

///|
pub fn LoroList::push(self : LoroList, _value : Value) -> Unit raise LoroError {
  raise LoroError("List is not implemented yet")
}

///|
pub fn LoroList::toArray(self : LoroList) -> Array[Value] {
  []
}

