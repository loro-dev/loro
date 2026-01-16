///|
pub struct LoroMovableList {
  doc : LoroDoc
  cid : @loro_codec.ContainerID
}

///|
pub fn LoroMovableList::asContainer(self : LoroMovableList) -> Container {
  Container::new(self.doc, self.cid)
}

///|
pub fn LoroMovableList::kind(self : LoroMovableList) -> @loro_codec.ContainerType {
  @loro_codec.ContainerType::movable_list()
}

///|
pub fn LoroMovableList::id(self : LoroMovableList) -> @loro_codec.ContainerID {
  self.cid
}

///|
pub fn LoroMovableList::isAttached(self : LoroMovableList) -> Bool {
  self.doc.has_container(self.cid)
}

///|
pub fn LoroMovableList::isDeleted(self : LoroMovableList) -> Bool {
  false
}

///|
pub fn LoroMovableList::getShallowValue(self : LoroMovableList) -> Json {
  self.doc.container_shallow_json(self.cid)
}

///|
pub fn LoroMovableList::toJSON(self : LoroMovableList) -> Json {
  self.doc.container_deep_json(self.cid)
}

///|
pub fn LoroMovableList::length(self : LoroMovableList) -> Int {
  0
}

///|
pub fn LoroMovableList::get(self : LoroMovableList, _index : Int) -> Value? {
  None
}

///|
pub fn LoroMovableList::insert(
  self : LoroMovableList,
  _index : Int,
  _value : Value,
) -> Unit raise LoroError {
  raise LoroError("MovableList is not implemented yet")
}

///|
pub fn LoroMovableList::delete(
  self : LoroMovableList,
  _index : Int,
  _len : Int,
) -> Unit raise LoroError {
  raise LoroError("MovableList is not implemented yet")
}

///|
pub fn LoroMovableList::push(
  self : LoroMovableList,
  _value : Value,
) -> Unit raise LoroError {
  raise LoroError("MovableList is not implemented yet")
}

///|
pub fn LoroMovableList::move(
  self : LoroMovableList,
  _from : Int,
  _to : Int,
) -> Unit raise LoroError {
  raise LoroError("MovableList is not implemented yet")
}

///|
pub fn LoroMovableList::set(
  self : LoroMovableList,
  _index : Int,
  _value : Value,
) -> Unit raise LoroError {
  raise LoroError("MovableList is not implemented yet")
}

