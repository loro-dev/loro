///|
pub struct LoroMovableList {
  doc : LoroDoc
  cid : @loro_codec.ContainerID
}

struct MovableListElem {
  elem_id : @loro_codec.IdLp
  mut value : Value
  mut value_id : @loro_codec.IdLp
  mut pos_id : @loro_codec.IdLp
  mut pos_item_id : @loro_codec.ID
}

struct MovableListState {
  tracker : SeqTracker
  // list item id -> elem_id
  item_to_elem : @hashmap.HashMap[String, @loro_codec.IdLp]
  // elem_id -> element
  elements : @hashmap.HashMap[String, MovableListElem]
}

fn idlp_string(idlp : @loro_codec.IdLp) -> String {
  idlp.lamport().to_string() + "@" + idlp.peer().to_string()
}

fn MovableListState::new() -> MovableListState {
  {
    tracker: SeqTracker::new(),
    item_to_elem: @hashmap.new(capacity=32),
    elements: @hashmap.new(capacity=32),
  }
}

fn MovableListState::op_len(self : MovableListState) -> Int {
  self.tracker.rope.active_len()
}

fn MovableListState::is_visible_item(self : MovableListState, item_id : @loro_codec.ID) -> Bool {
  let item_k = id_string(item_id)
  match self.item_to_elem.get(item_k) {
    None => false
    Some(elem_id) =>
      match self.elements.get(idlp_string(elem_id)) {
        None => false
        Some(e) => e.pos_item_id == item_id
      }
  }
}

fn MovableListState::user_len(self : MovableListState) -> Int {
  let mut user_len = 0
  for s in self.tracker.rope.spans {
    if !s.status.is_activated() {
      continue
    }
    let l = s.rle_len()
    for i in 0..<l {
      if self.is_visible_item(s.id.inc(i).id()) {
        user_len = user_len + 1
      }
    }
  }
  user_len
}

fn MovableListState::user_to_op_index(self : MovableListState, user_index : Int) -> Int raise LoroError {
  if user_index < 0 {
    raise LoroError("movable_list: negative index")
  }
  let mut op_index = 0
  let mut seen = 0
  for s in self.tracker.rope.spans {
    if !s.status.is_activated() {
      continue
    }
    let l = s.rle_len()
    for i in 0..<l {
      let item_id = s.id.inc(i).id()
      if self.is_visible_item(item_id) {
        if seen == user_index {
          return op_index
        }
        seen = seen + 1
      }
      op_index = op_index + 1
    }
  }
  if seen == user_index {
    op_index
  } else {
    raise LoroError("movable_list: index out of range")
  }
}

fn MovableListState::user_elem_id_at(self : MovableListState, user_index : Int) -> @loro_codec.IdLp? {
  if user_index < 0 {
    return None
  }
  let mut seen = 0
  for s in self.tracker.rope.spans {
    if !s.status.is_activated() {
      continue
    }
    let l = s.rle_len()
    for i in 0..<l {
      let item_id = s.id.inc(i).id()
      if !self.is_visible_item(item_id) {
        continue
      }
      if seen == user_index {
        match self.item_to_elem.get(id_string(item_id)) {
          None => return None
          Some(eid) => return Some(eid)
        }
      }
      seen = seen + 1
    }
  }
  None
}

fn MovableListState::user_value_at(self : MovableListState, user_index : Int) -> Value? {
  match self.user_elem_id_at(user_index) {
    None => None
    Some(eid) =>
      match self.elements.get(idlp_string(eid)) {
        None => None
        Some(e) => Some(e.value)
      }
  }
}

fn MovableListState::user_range_to_op_range(
  self : MovableListState,
  user_index : Int,
  user_len : Int,
) -> (Int, Int) raise LoroError {
  if user_len <= 0 {
    return (0, 0)
  }
  let start_op = self.user_to_op_index(user_index)
  let mut op_index = 0
  let mut started = false
  let mut scanned = 0
  let mut seen = 0
  for s in self.tracker.rope.spans {
    if !s.status.is_activated() {
      continue
    }
    let l = s.rle_len()
    for i in 0..<l {
      let item_id = s.id.inc(i).id()
      if !started {
        if op_index == start_op {
          started = true
        } else {
          op_index = op_index + 1
          continue
        }
      }

      scanned = scanned + 1
      if self.is_visible_item(item_id) {
        seen = seen + 1
        if seen == user_len {
          return (start_op, scanned)
        }
      }
      op_index = op_index + 1
    }
  }
  raise LoroError("movable_list: range out of bounds")
}

fn LoroDoc::ensure_movable_list_state(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
) -> MovableListState {
  let key = container_id_string(cid)
  match self.inner.movable_list_states.get(key) {
    Some(s) => s
    None => {
      let s = MovableListState::new()
      self.inner.movable_list_states.set(key, s)
      s
    }
  }
}

fn LoroDoc::movable_list_len(self : LoroDoc, cid : @loro_codec.ContainerID) -> Int {
  let s = self.ensure_movable_list_state(cid)
  s.user_len()
}

fn LoroDoc::movable_list_get(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  index : Int,
) -> Value? {
  let s = self.ensure_movable_list_state(cid)
  s.user_value_at(index)
}

fn LoroDoc::movable_list_to_array(self : LoroDoc, cid : @loro_codec.ContainerID) -> Array[Value] {
  let s = self.ensure_movable_list_state(cid)
  let out : Array[Value] = []
  for i in 0..<s.user_len() {
    match s.user_value_at(i) {
      None => ()
      Some(v) => out.push(v)
    }
  }
  out
}

fn LoroDoc::movable_list_shallow_json(self : LoroDoc, cid : @loro_codec.ContainerID) -> Json {
  let arr : Array[Json] = []
  let items = self.movable_list_to_array(cid)
  for v in items {
    arr.push(value_to_json_shallow(v))
  }
  Json::array(arr)
}

fn LoroDoc::movable_list_deep_json(self : LoroDoc, cid : @loro_codec.ContainerID) -> Json {
  let arr : Array[Json] = []
  let items = self.movable_list_to_array(cid)
  for v in items {
    arr.push(self.value_deep_json(v))
  }
  Json::array(arr)
}

fn LoroDoc::movable_list_insert(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  index : Int,
  value : Value,
) -> Unit raise LoroError {
  let key = container_id_string(cid)
  if value_contains_container(value) {
    raise LoroError("movable_list.insert: container values require insertContainer()")
  }
  let s = self.ensure_movable_list_state(cid)
  let cur_len = s.user_len()
  if index < 0 || index > cur_len {
    raise LoroError("movable_list.insert: index out of range")
  }
  let op_pos = s.user_to_op_index(index)

  self.start_txn_if_needed()
  let peer = self.inner.peer
  let counter = self.inner.txn_next_counter
  let lamport = self.inner.txn_next_lamport

  let op_id = SeqIdFull::new(peer, counter, lamport)
  s.tracker.insert(op_id, op_pos, SeqContent::Unknown(1))
  let item_id = @loro_codec.ID::new(peer, counter)
  let elem_id = @loro_codec.IdLp::new(peer, lamport)
  s.item_to_elem.set(id_string(item_id), elem_id)
  s.elements.set(idlp_string(elem_id), {
    elem_id,
    value,
    value_id: elem_id,
    pos_id: elem_id,
    pos_item_id: item_id,
  })
  self.inner.movable_list_states.set(key, s)

  self.inner.pending_ops.push({
    container: cid,
    counter,
    lamport,
    len: 1U,
    content: PendingOpContent::MovableListInsert(op_pos, [value]),
  })
  self.inner.txn_next_counter = counter + 1
  self.inner.txn_next_lamport = lamport + 1U
}

fn LoroDoc::movable_list_insert_container(
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
    _ => raise LoroError("movable_list.insertContainer: unsupported container type")
  }
  let s = self.ensure_movable_list_state(cid)
  let cur_len = s.user_len()
  if index < 0 || index > cur_len {
    raise LoroError("movable_list.insertContainer: index out of range")
  }
  let op_pos = s.user_to_op_index(index)

  self.start_txn_if_needed()
  let peer = self.inner.peer
  let counter = self.inner.txn_next_counter
  let lamport = self.inner.txn_next_lamport
  let child = @loro_codec.ContainerID::normal(peer, counter, kind)
  self.ensure_container_state(child)
  let value = Value::Container(child)

  let op_id = SeqIdFull::new(peer, counter, lamport)
  s.tracker.insert(op_id, op_pos, SeqContent::Unknown(1))
  let item_id = @loro_codec.ID::new(peer, counter)
  let elem_id = @loro_codec.IdLp::new(peer, lamport)
  s.item_to_elem.set(id_string(item_id), elem_id)
  s.elements.set(idlp_string(elem_id), {
    elem_id,
    value,
    value_id: elem_id,
    pos_id: elem_id,
    pos_item_id: item_id,
  })
  self.inner.movable_list_states.set(key, s)

  self.inner.pending_ops.push({
    container: cid,
    counter,
    lamport,
    len: 1U,
    content: PendingOpContent::MovableListInsert(op_pos, [value]),
  })
  self.inner.txn_next_counter = counter + 1
  self.inner.txn_next_lamport = lamport + 1U
  Container::new(self, child)
}

fn LoroDoc::movable_list_delete(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  index : Int,
  len : Int,
) -> Unit raise LoroError {
  let key = container_id_string(cid)
  if len <= 0 {
    return
  }
  let s = self.ensure_movable_list_state(cid)
  let cur_len = s.user_len()
  if index < 0 || index + len > cur_len {
    raise LoroError("movable_list.delete: range out of bounds")
  }

  let (op_pos, op_len) = s.user_range_to_op_range(index, len)
  if op_len <= 0 {
    return
  }
  let start_id = s.tracker.rope.id_at_active_index(op_pos)
  let deleted_item_ids : Array[@loro_codec.ID] = []
  for i in 0..<op_len {
    deleted_item_ids.push(s.tracker.rope.id_at_active_index(op_pos + i))
  }

  self.start_txn_if_needed()
  let peer = self.inner.peer
  let counter = self.inner.txn_next_counter
  let lamport = self.inner.txn_next_lamport
  s.tracker.delete(@loro_codec.ID::new(peer, counter), start_id, op_pos, op_len, false)

  for del_id in deleted_item_ids {
    match s.item_to_elem.get(id_string(del_id)) {
      None => ()
      Some(eid) => {
        let ek = idlp_string(eid)
        match s.elements.get(ek) {
          None => ()
          Some(e) =>
            if e.pos_item_id == del_id {
              s.elements.remove(ek)
            }
        }
      }
    }
  }

  self.inner.pending_ops.push({
    container: cid,
    counter,
    lamport,
    len: op_len.reinterpret_as_uint(),
    content: PendingOpContent::MovableListDelete(op_pos, op_len, start_id),
  })
  self.inner.txn_next_counter = counter + op_len
  self.inner.txn_next_lamport = lamport + op_len.reinterpret_as_uint()
  self.inner.movable_list_states.set(key, s)
}

fn LoroDoc::movable_list_move(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  from : Int,
  to : Int,
) -> Unit raise LoroError {
  let key = container_id_string(cid)
  if from == to {
    return
  }
  let s = self.ensure_movable_list_state(cid)
  let cur_len = s.user_len()
  if from < 0 || from >= cur_len || to < 0 || to >= cur_len {
    raise LoroError("movable_list.move: index out of range")
  }
  let from_op = s.user_to_op_index(from)
  let to_op = s.user_to_op_index(to)
  let elem_id = match s.user_elem_id_at(from) {
    None => raise LoroError("movable_list.move: missing elem_id")
    Some(eid) => eid
  }
  let deleted_id = s.tracker.rope.id_at_active_index(from_op)

  self.start_txn_if_needed()
  let peer = self.inner.peer
  let counter = self.inner.txn_next_counter
  let lamport = self.inner.txn_next_lamport
  let op_id = SeqIdFull::new(peer, counter, lamport)
  s.tracker.move_item(op_id, deleted_id, from_op, to_op)

  let new_item_id = @loro_codec.ID::new(peer, counter)
  s.item_to_elem.set(id_string(new_item_id), elem_id)
  match s.elements.get(idlp_string(elem_id)) {
    None => ()
    Some(e) => {
      let new_pos_id = @loro_codec.IdLp::new(peer, lamport)
      if idlp_gt(new_pos_id, e.pos_id) {
        e.pos_id = new_pos_id
        e.pos_item_id = new_item_id
      }
    }
  }

  self.inner.pending_ops.push({
    container: cid,
    counter,
    lamport,
    len: 1U,
    content: PendingOpContent::MovableListMove(from_op, to_op, elem_id),
  })
  self.inner.txn_next_counter = counter + 1
  self.inner.txn_next_lamport = lamport + 1U
  self.inner.movable_list_states.set(key, s)
}

fn LoroDoc::movable_list_set(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  index : Int,
  value : Value,
) -> Unit raise LoroError {
  let key = container_id_string(cid)
  if value_contains_container(value) {
    raise LoroError("movable_list.set: container values require setContainer()")
  }
  let s = self.ensure_movable_list_state(cid)
  let cur_len = s.user_len()
  if index < 0 || index >= cur_len {
    raise LoroError("movable_list.set: index out of range")
  }
  let elem_id = match s.user_elem_id_at(index) {
    None => raise LoroError("movable_list.set: missing elem_id")
    Some(eid) => eid
  }

  self.start_txn_if_needed()
  let peer = self.inner.peer
  let counter = self.inner.txn_next_counter
  let lamport = self.inner.txn_next_lamport
  let new_value_id = @loro_codec.IdLp::new(peer, lamport)
  match s.elements.get(idlp_string(elem_id)) {
    None => ()
    Some(e) =>
      if idlp_gt(new_value_id, e.value_id) {
        e.value = value
        e.value_id = new_value_id
      }
  }

  self.inner.pending_ops.push({
    container: cid,
    counter,
    lamport,
    len: 1U,
    content: PendingOpContent::MovableListSet(elem_id, value),
  })
  self.inner.txn_next_counter = counter + 1
  self.inner.txn_next_lamport = lamport + 1U
  self.inner.movable_list_states.set(key, s)
}

fn LoroDoc::movable_list_set_container(
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
    _ => raise LoroError("movable_list.setContainer: unsupported container type")
  }
  let s = self.ensure_movable_list_state(cid)
  let cur_len = s.user_len()
  if index < 0 || index >= cur_len {
    raise LoroError("movable_list.setContainer: index out of range")
  }
  let elem_id = match s.user_elem_id_at(index) {
    None => raise LoroError("movable_list.setContainer: missing elem_id")
    Some(eid) => eid
  }

  self.start_txn_if_needed()
  let peer = self.inner.peer
  let counter = self.inner.txn_next_counter
  let lamport = self.inner.txn_next_lamport
  let child = @loro_codec.ContainerID::normal(peer, counter, kind)
  self.ensure_container_state(child)
  let value = Value::Container(child)
  let new_value_id = @loro_codec.IdLp::new(peer, lamport)
  match s.elements.get(idlp_string(elem_id)) {
    None => ()
    Some(e) =>
      if idlp_gt(new_value_id, e.value_id) {
        e.value = value
        e.value_id = new_value_id
      }
  }

  self.inner.pending_ops.push({
    container: cid,
    counter,
    lamport,
    len: 1U,
    content: PendingOpContent::MovableListSet(elem_id, value),
  })
  self.inner.txn_next_counter = counter + 1
  self.inner.txn_next_lamport = lamport + 1U
  self.inner.movable_list_states.set(key, s)
  Container::new(self, child)
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
  self.doc.movable_list_len(self.cid)
}

///|
pub fn LoroMovableList::get(self : LoroMovableList, _index : Int) -> Value? {
  self.doc.movable_list_get(self.cid, _index)
}

///|
pub fn LoroMovableList::insert(
  self : LoroMovableList,
  _index : Int,
  _value : Value,
) -> Unit raise LoroError {
  self.doc.movable_list_insert(self.cid, _index, _value)
}

///|
pub fn LoroMovableList::delete(
  self : LoroMovableList,
  _index : Int,
  _len : Int,
) -> Unit raise LoroError {
  self.doc.movable_list_delete(self.cid, _index, _len)
}

///|
pub fn LoroMovableList::push(
  self : LoroMovableList,
  _value : Value,
) -> Unit raise LoroError {
  let n = self.length()
  self.doc.movable_list_insert(self.cid, n, _value)
}

///|
pub fn LoroMovableList::move(
  self : LoroMovableList,
  _from : Int,
  _to : Int,
) -> Unit raise LoroError {
  self.doc.movable_list_move(self.cid, _from, _to)
}

///|
pub fn LoroMovableList::set(
  self : LoroMovableList,
  _index : Int,
  _value : Value,
) -> Unit raise LoroError {
  self.doc.movable_list_set(self.cid, _index, _value)
}

///|
pub fn LoroMovableList::toArray(self : LoroMovableList) -> Array[Value] {
  self.doc.movable_list_to_array(self.cid)
}

///|
pub fn LoroMovableList::insertContainer(
  self : LoroMovableList,
  index : Int,
  child : Container,
) -> Container raise LoroError {
  self.doc.movable_list_insert_container(self.cid, index, child.kind())
}

///|
pub fn LoroMovableList::pushContainer(
  self : LoroMovableList,
  child : Container,
) -> Container raise LoroError {
  let n = self.length()
  self.doc.movable_list_insert_container(self.cid, n, child.kind())
}

///|
pub fn LoroMovableList::setContainer(
  self : LoroMovableList,
  index : Int,
  child : Container,
) -> Container raise LoroError {
  self.doc.movable_list_set_container(self.cid, index, child.kind())
}
