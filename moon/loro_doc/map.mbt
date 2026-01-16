struct MapEntry {
  value : Value?
  writer : @loro_codec.IdLp
}

struct MapState {
  entries : @hashmap.HashMap[String, MapEntry]
}

fn MapState::new() -> MapState {
  { entries: @hashmap.new(capacity=8) }
}

fn MapState::get(self : MapState, key : String) -> Value? {
  match self.entries.get(key) {
    None => None
    Some(entry) => entry.value
  }
}

fn MapState::has(self : MapState, key : String) -> Bool {
  match self.entries.get(key) {
    None => false
    Some(entry) => match entry.value {
      None => false
      Some(_) => true
    }
  }
}

fn MapState::set(
  self : MapState,
  key : String,
  value : Value,
  writer : @loro_codec.IdLp,
) -> Unit {
  match self.entries.get(key) {
    None => self.entries.set(key, { value: Some(value), writer })
    Some(entry) =>
      if idlp_gt(writer, entry.writer) {
        self.entries.set(key, { value: Some(value), writer })
      }
  }
}

fn MapState::delete(self : MapState, key : String, writer : @loro_codec.IdLp) -> Unit {
  match self.entries.get(key) {
    None => self.entries.set(key, { value: None, writer })
    Some(entry) =>
      if idlp_gt(writer, entry.writer) {
        self.entries.set(key, { value: None, writer })
      }
  }
}

fn LoroDoc::ensure_map_state(self : LoroDoc, cid : @loro_codec.ContainerID) -> MapState {
  let key = container_id_string(cid)
  match self.inner.map_states.get(key) {
    Some(s) => s
    None => {
      let s = MapState::new()
      self.inner.map_states.set(key, s)
      s
    }
  }
}

fn LoroDoc::apply_map_set(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  key : String,
  value : Value,
  writer : @loro_codec.IdLp,
) -> Unit {
  let state = self.ensure_map_state(cid)
  state.set(key, value, writer)
  match value {
    Value::Container(child) =>
      match child {
        @loro_codec.ContainerID::Root(_name, _kind) => ()
        @loro_codec.ContainerID::Normal(_peer, _counter, kind) =>
          match kind {
            @loro_codec.ContainerType::Map => {
              let _ = self.ensure_map_state(child)
            }
            _ => ()
          }
      }
    _ => ()
  }
}

fn LoroDoc::apply_map_delete(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  key : String,
  writer : @loro_codec.IdLp,
) -> Unit {
  let state = self.ensure_map_state(cid)
  state.delete(key, writer)
}

fn LoroDoc::value_deep_json(self : LoroDoc, value : Value) -> Json {
  match value {
    Value::Container(cid) => self.container_deep_json(cid)
    _ => value_to_json_shallow(value)
  }
}

fn LoroDoc::map_shallow_json(self : LoroDoc, cid : @loro_codec.ContainerID) -> Json {
  let key = container_id_string(cid)
  let obj = Map::new(capacity=16)
  match self.inner.map_states.get(key) {
    None => ()
    Some(state) =>
      for pair in state.entries {
        let (k, entry) = pair
        match entry.value {
          None => ()
          Some(v) => obj[k] = value_to_json_shallow(v)
        }
      }
  }
  Json::object(obj)
}

fn LoroDoc::map_deep_json(self : LoroDoc, cid : @loro_codec.ContainerID) -> Json {
  let key = container_id_string(cid)
  let obj = Map::new(capacity=16)
  match self.inner.map_states.get(key) {
    None => ()
    Some(state) =>
      for pair in state.entries {
        let (k, entry) = pair
        match entry.value {
          None => ()
          Some(v) => obj[k] = self.value_deep_json(v)
        }
      }
  }
  Json::object(obj)
}

fn LoroDoc::map_get(self : LoroDoc, cid : @loro_codec.ContainerID, key : String) -> Value? {
  let state = self.ensure_map_state(cid)
  state.get(key)
}

fn LoroDoc::map_has(self : LoroDoc, cid : @loro_codec.ContainerID, key : String) -> Bool {
  let state = self.ensure_map_state(cid)
  state.has(key)
}

fn LoroDoc::map_keys(self : LoroDoc, cid : @loro_codec.ContainerID) -> Array[String] {
  let state = self.ensure_map_state(cid)
  let out : Array[String] = []
  for pair in state.entries {
    let (k, entry) = pair
    match entry.value {
      None => ()
      Some(_) => out.push(k)
    }
  }
  out.sort()
  out
}

fn LoroDoc::map_entries(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
) -> Array[(String, Value)] {
  let state = self.ensure_map_state(cid)
  let out : Array[(String, Value)] = []
  for pair in state.entries {
    let (k, entry) = pair
    match entry.value {
      None => ()
      Some(v) => out.push((k, v))
    }
  }
  out
}

fn LoroDoc::map_set(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  key : String,
  value : Value,
) -> Unit raise LoroError {
  if value_contains_container(value) {
    raise LoroError("map.set: container values require setContainer()")
  }
  self.start_txn_if_needed()
  let peer = self.inner.peer
  let counter = self.inner.txn_next_counter
  let lamport = self.inner.txn_next_lamport
  let writer = @loro_codec.IdLp::new(peer, lamport)
  self.apply_map_set(cid, key, value, writer)
  self.inner.pending_ops.push({
    container: cid,
    counter,
    lamport,
    len: 1U,
    content: PendingOpContent::MapInsert(key, value),
  })
  self.inner.txn_next_counter = counter + 1
  self.inner.txn_next_lamport = lamport + 1U
}

fn LoroDoc::map_delete(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  key : String,
) -> Unit {
  // Local edits must be visible immediately; assign an id now.
  self.start_txn_if_needed() catch { LoroError(_) => () }
  let peer = self.inner.peer
  let counter = self.inner.txn_next_counter
  let lamport = self.inner.txn_next_lamport
  let writer = @loro_codec.IdLp::new(peer, lamport)
  self.apply_map_delete(cid, key, writer)
  self.inner.pending_ops.push({
    container: cid,
    counter,
    lamport,
    len: 1U,
    content: PendingOpContent::MapDelete(key),
  })
  self.inner.txn_next_counter = counter + 1
  self.inner.txn_next_lamport = lamport + 1U
}

///|
pub struct LoroMap {
  doc : LoroDoc
  cid : @loro_codec.ContainerID
}

///|
pub fn LoroMap::asContainer(self : LoroMap) -> Container {
  Container::new(self.doc, self.cid)
}

///|
pub fn LoroMap::kind(self : LoroMap) -> @loro_codec.ContainerType {
  @loro_codec.ContainerType::map()
}

///|
pub fn LoroMap::id(self : LoroMap) -> @loro_codec.ContainerID {
  self.cid
}

///|
pub fn LoroMap::isAttached(self : LoroMap) -> Bool {
  self.doc.has_container(self.cid)
}

///|
pub fn LoroMap::isDeleted(self : LoroMap) -> Bool {
  false
}

///|
pub fn LoroMap::getShallowValue(self : LoroMap) -> Json {
  self.doc.map_shallow_json(self.cid)
}

///|
pub fn LoroMap::toJSON(self : LoroMap) -> Json {
  self.doc.map_deep_json(self.cid)
}

///|
pub fn LoroMap::get(self : LoroMap, key : String) -> Value? {
  self.doc.map_get(self.cid, key)
}

///|
pub fn LoroMap::set(
  self : LoroMap,
  key : String,
  value : Value,
) -> Unit raise LoroError {
  self.doc.map_set(self.cid, key, value)
}

///|
pub fn LoroMap::delete(self : LoroMap, key : String) -> Unit {
  self.doc.map_delete(self.cid, key)
}

///|
pub fn LoroMap::has(self : LoroMap, key : String) -> Bool {
  self.doc.map_has(self.cid, key)
}

///|
pub fn LoroMap::keys(self : LoroMap) -> Array[String] {
  self.doc.map_keys(self.cid)
}

///|
pub fn LoroMap::entries(self : LoroMap) -> Array[(String, Value)] {
  self.doc.map_entries(self.cid)
}
