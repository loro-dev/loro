///|
pub struct LoroList {
  doc : LoroDoc
  cid : @loro_codec.ContainerID
}

fn LoroDoc::ensure_list_tracker(self : LoroDoc, cid : @loro_codec.ContainerID) -> SeqTracker {
  let key = container_id_string(cid)
  match self.inner.list_trackers.get(key) {
    Some(t) => t
    None => {
      let t = SeqTracker::new()
      self.inner.list_trackers.set(key, t)
      t
    }
  }
}

fn LoroDoc::list_len(self : LoroDoc, cid : @loro_codec.ContainerID) -> Int {
  let t = self.ensure_list_tracker(cid)
  t.rope.active_len()
}

fn LoroDoc::list_get(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  index : Int,
) -> Value? {
  if index < 0 {
    return None
  }
  let t = self.ensure_list_tracker(cid)
  let mut left = index
  for s in t.rope.spans {
    if !s.status.is_activated() {
      continue
    }
    match s.content {
      SeqContent::List(items) => {
        if left < items.length() {
          return Some(items[left])
        }
        left = left - items.length()
      }
      SeqContent::Unknown(n) => left = left - n
      SeqContent::MoveAnchor => left = left - 1
      SeqContent::TextStyleAnchor(_) => left = left - 1
      SeqContent::Text(_, n) => left = left - n
    }
  }
  None
}

fn LoroDoc::list_to_array(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
) -> Array[Value] {
  let t = self.ensure_list_tracker(cid)
  let out : Array[Value] = []
  for s in t.rope.spans {
    if !s.status.is_activated() {
      continue
    }
    match s.content {
      SeqContent::List(items) =>
        for v in items {
          out.push(v)
        }
      _ => ()
    }
  }
  out
}

fn LoroDoc::list_shallow_json(self : LoroDoc, cid : @loro_codec.ContainerID) -> Json {
  let arr : Array[Json] = []
  let items = self.list_to_array(cid)
  for v in items {
    arr.push(value_to_json_shallow(v))
  }
  Json::array(arr)
}

fn LoroDoc::list_deep_json(self : LoroDoc, cid : @loro_codec.ContainerID) -> Json {
  let arr : Array[Json] = []
  let items = self.list_to_array(cid)
  for v in items {
    arr.push(self.value_deep_json(v))
  }
  Json::array(arr)
}

fn LoroDoc::list_insert(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  index : Int,
  value : Value,
) -> Unit raise LoroError {
  let key = container_id_string(cid)
  if value_contains_container(value) {
    raise LoroError("list.insert: container values require insertContainer()")
  }
  if index < 0 {
    raise LoroError("list.insert: negative index")
  }
  let t = self.ensure_list_tracker(cid)
  let cur_len = t.rope.active_len()
  if index > cur_len {
    raise LoroError("list.insert: index out of range")
  }

  self.start_txn_if_needed()
  let peer = self.inner.peer
  let counter = self.inner.txn_next_counter
  let lamport = self.inner.txn_next_lamport
  t.insert(
    SeqIdFull::new(peer, counter, lamport),
    index,
    SeqContent::List([value]),
  )
  self.inner.list_trackers.set(key, t)

  self.inner.pending_ops.push({
    container: cid,
    counter,
    lamport,
    len: 1U,
    content: PendingOpContent::ListInsert(index, [value]),
  })
  self.inner.txn_next_counter = counter + 1
  self.inner.txn_next_lamport = lamport + 1U
}

fn LoroDoc::list_insert_container(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  index : Int,
  kind : @loro_codec.ContainerType,
) -> Container raise LoroError {
  let key = container_id_string(cid)
  match kind {
    @loro_codec.ContainerType::Map => ()
    @loro_codec.ContainerType::List => ()
    @loro_codec.ContainerType::MovableList => ()
    @loro_codec.ContainerType::Text => ()
    @loro_codec.ContainerType::Tree => ()
    _ => raise LoroError("list.insertContainer: unsupported container type")
  }
  if index < 0 {
    raise LoroError("list.insertContainer: negative index")
  }
  let t = self.ensure_list_tracker(cid)
  let cur_len = t.rope.active_len()
  if index > cur_len {
    raise LoroError("list.insertContainer: index out of range")
  }

  self.start_txn_if_needed()
  let peer = self.inner.peer
  let counter = self.inner.txn_next_counter
  let lamport = self.inner.txn_next_lamport
  let child = @loro_codec.ContainerID::normal(peer, counter, kind)
  self.ensure_container_state(child)
  let value = Value::Container(child)
  t.insert(
    SeqIdFull::new(peer, counter, lamport),
    index,
    SeqContent::List([value]),
  )
  self.inner.list_trackers.set(key, t)

  self.inner.pending_ops.push({
    container: cid,
    counter,
    lamport,
    len: 1U,
    content: PendingOpContent::ListInsert(index, [value]),
  })
  self.inner.txn_next_counter = counter + 1
  self.inner.txn_next_lamport = lamport + 1U
  Container::new(self, child)
}

fn LoroDoc::list_delete(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  index : Int,
  len : Int,
) -> Unit raise LoroError {
  let key = container_id_string(cid)
  if len <= 0 {
    return
  }
  if index < 0 {
    raise LoroError("list.delete: negative index")
  }
  let t = self.ensure_list_tracker(cid)
  let cur_len = t.rope.active_len()
  if index + len > cur_len {
    raise LoroError("list.delete: range out of bounds")
  }

  let start_id = t.rope.id_at_active_index(index)
  self.start_txn_if_needed()
  let peer = self.inner.peer
  let counter = self.inner.txn_next_counter
  let lamport = self.inner.txn_next_lamport
  t.delete(@loro_codec.ID::new(peer, counter), start_id, index, len, false)
  self.inner.list_trackers.set(key, t)

  self.inner.pending_ops.push({
    container: cid,
    counter,
    lamport,
    len: len.reinterpret_as_uint(),
    content: PendingOpContent::ListDelete(index, len, start_id),
  })
  self.inner.txn_next_counter = counter + len
  self.inner.txn_next_lamport = lamport + len.reinterpret_as_uint()
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
  self.doc.list_len(self.cid)
}

///|
pub fn LoroList::get(self : LoroList, _index : Int) -> Value? {
  self.doc.list_get(self.cid, _index)
}

///|
pub fn LoroList::insert(
  self : LoroList,
  index : Int,
  value : Value,
) -> Unit raise LoroError {
  self.doc.list_insert(self.cid, index, value)
}

///|
pub fn LoroList::delete(self : LoroList, index : Int, len : Int) -> Unit raise LoroError {
  self.doc.list_delete(self.cid, index, len)
}

///|
pub fn LoroList::push(self : LoroList, _value : Value) -> Unit raise LoroError {
  let n = self.length()
  self.doc.list_insert(self.cid, n, _value)
}

///|
pub fn LoroList::toArray(self : LoroList) -> Array[Value] {
  self.doc.list_to_array(self.cid)
}

///|
pub fn LoroList::insertContainer(
  self : LoroList,
  index : Int,
  child : Container,
) -> Container raise LoroError {
  self.doc.list_insert_container(self.cid, index, child.kind())
}

///|
pub fn LoroList::pushContainer(self : LoroList, child : Container) -> Container raise LoroError {
  let n = self.length()
  self.doc.list_insert_container(self.cid, n, child.kind())
}
