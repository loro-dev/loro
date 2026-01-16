///|
pub struct LoroText {
  doc : LoroDoc
  cid : @loro_codec.ContainerID
}

fn LoroDoc::ensure_text_tracker(self : LoroDoc, cid : @loro_codec.ContainerID) -> SeqTracker {
  let key = container_id_string(cid)
  match self.inner.text_trackers.get(key) {
    Some(t) => t
    None => {
      let t = SeqTracker::new()
      self.inner.text_trackers.set(key, t)
      t
    }
  }
}

fn LoroDoc::text_len(self : LoroDoc, cid : @loro_codec.ContainerID) -> Int {
  let t = self.ensure_text_tracker(cid)
  t.rope.active_len()
}

fn LoroDoc::text_to_string(self : LoroDoc, cid : @loro_codec.ContainerID) -> String {
  let t = self.ensure_text_tracker(cid)
  let mut out = ""
  for s in t.rope.spans {
    if !s.status.is_activated() {
      continue
    }
    match s.content {
      SeqContent::Text(str, _n) => out = out + str
      _ => ()
    }
  }
  out
}

fn LoroDoc::text_insert(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  index : Int,
  text : String,
) -> Unit raise LoroError {
  if index < 0 {
    raise LoroError("text.insert: negative index")
  }
  let t = self.ensure_text_tracker(cid)
  let cur_len = t.rope.active_len()
  if index > cur_len {
    raise LoroError("text.insert: index out of range")
  }
  let n = utf8_count_chars(text)
  if n == 0 {
    return
  }

  self.start_txn_if_needed()
  let peer = self.inner.peer
  let counter = self.inner.txn_next_counter
  let lamport = self.inner.txn_next_lamport
  t.insert(SeqIdFull::new(peer, counter, lamport), index, SeqContent::Text(text, n))

  self.inner.pending_ops.push({
    container: cid,
    counter,
    lamport,
    len: n.reinterpret_as_uint(),
    content: PendingOpContent::TextInsert(index, text),
  })
  self.inner.txn_next_counter = counter + n
  self.inner.txn_next_lamport = lamport + n.reinterpret_as_uint()
}

fn LoroDoc::text_delete(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  index : Int,
  len : Int,
) -> Unit raise LoroError {
  if len <= 0 {
    return
  }
  if index < 0 {
    raise LoroError("text.delete: negative index")
  }
  let t = self.ensure_text_tracker(cid)
  let cur_len = t.rope.active_len()
  if index + len > cur_len {
    raise LoroError("text.delete: range out of bounds")
  }
  let start_id = t.rope.id_at_active_index(index)

  self.start_txn_if_needed()
  let peer = self.inner.peer
  let counter = self.inner.txn_next_counter
  let lamport = self.inner.txn_next_lamport
  t.delete(@loro_codec.ID::new(peer, counter), start_id, index, len, false)

  self.inner.pending_ops.push({
    container: cid,
    counter,
    lamport,
    len: len.reinterpret_as_uint(),
    content: PendingOpContent::TextDelete(index, len, start_id),
  })
  self.inner.txn_next_counter = counter + len
  self.inner.txn_next_lamport = lamport + len.reinterpret_as_uint()
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
  self.doc.text_len(self.cid)
}

///|
pub fn LoroText::toString(self : LoroText) -> String {
  self.doc.text_to_string(self.cid)
}

///|
pub fn LoroText::insert(self : LoroText, index : Int, text : String) -> Unit raise LoroError {
  self.doc.text_insert(self.cid, index, text)
}

///|
pub fn LoroText::delete(self : LoroText, index : Int, len : Int) -> Unit raise LoroError {
  self.doc.text_delete(self.cid, index, len)
}
