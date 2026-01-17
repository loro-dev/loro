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

fn value_is_null(v : Value) -> Bool {
  match v {
    Value::Null => true
    _ => false
  }
}

fn value_is_false(v : Value) -> Bool {
  match v {
    Value::False => true
    _ => false
  }
}

fn best_insert_pos_in_style_anchors(
  anchors : Array[(TextStyleAnchor, Int)],
  base_entity_pos : Int,
) -> Int {
  if anchors.length() == 0 {
    return base_entity_pos
  }

  let visited : Array[(TextStyleAnchor, Int)] = []
  let mut chosen : Int? = None
  for pair in anchors {
    let (a, pos) = pair
    visited.push((a, pos))
    // Rule 1: insert before style anchors that contain the beginning of new marks.
    if a.anchor_type == TextAnchorType::Start &&
      (!value_is_null(a.value) || !value_is_false(a.value)) {
      chosen = Some(pos)
      break
    }
    // Rule 2: insert before style anchors that prefer inserting before.
    if text_style_info_prefer_insert_before(a.info, a.anchor_type) {
      chosen = Some(pos)
      break
    }
  }

  let mut entity_pos =
    match chosen {
      Some(p) => p
      None => base_entity_pos + anchors.length()
    }

  // Rule 3: insert after style anchors that prefer inserting after.
  while visited.length() > 0 {
    let (a, pos) = visited[visited.length() - 1]
    let _ = visited.pop()
    if !text_style_info_prefer_insert_before(a.info, a.anchor_type) {
      break
    }
    entity_pos = pos
  }
  entity_pos
}

fn LoroDoc::text_unicode_len(self : LoroDoc, cid : @loro_codec.ContainerID) -> Int {
  let t = self.ensure_text_tracker(cid)
  let mut sum = 0
  for s in t.rope.spans {
    if !s.status.is_activated() {
      continue
    }
    match s.content {
      SeqContent::Text(_str, n) => sum = sum + n
      _ => ()
    }
  }
  sum
}

fn LoroDoc::text_entity_index_for_insert(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  unicode_pos : Int,
) -> Int raise LoroError {
  if unicode_pos < 0 {
    raise LoroError("text index: negative index")
  }
  let t = self.ensure_text_tracker(cid)
  let unicode_len = self.text_unicode_len(cid)
  if unicode_pos > unicode_len {
    raise LoroError("text index: out of range")
  }
  if t.rope.spans.length() == 0 {
    return 0
  }

  if unicode_pos == 0 {
    let anchors : Array[(TextStyleAnchor, Int)] = []
    let mut ent = 0
    for s in t.rope.spans {
      if !s.status.is_activated() {
        continue
      }
      match s.content {
        SeqContent::Text(_str, _n) =>
          if anchors.length() == 0 {
            return 0
          } else {
            break
          }
        SeqContent::TextStyleAnchor(a) => {
          anchors.push((a, ent))
          ent = ent + 1
        }
        _ => break
      }
    }
    return best_insert_pos_in_style_anchors(anchors, 0)
  }

  let target_char = unicode_pos - 1
  let mut uni = 0
  let mut ent = 0
  let spans = t.rope.spans
  let mut i = 0
  while i < spans.length() {
    let s = spans[i]
    if !s.status.is_activated() {
      i = i + 1
      continue
    }
    match s.content {
      SeqContent::Text(_str, n) => {
        if target_char < uni + n {
          let offset = target_char - uni
          let after = offset + 1
          let base = ent + after
          if after < n {
            return base
          }

          let anchors : Array[(TextStyleAnchor, Int)] = []
          let mut ent2 = base
          let mut j = i + 1
          while j < spans.length() {
            let s2 = spans[j]
            if !s2.status.is_activated() {
              j = j + 1
              continue
            }
            match s2.content {
              SeqContent::TextStyleAnchor(a) => {
                anchors.push((a, ent2))
                ent2 = ent2 + 1
              }
              SeqContent::Text(_str2, _n2) => break
              _ => break
            }
            j = j + 1
          }
          if anchors.length() == 0 {
            return base
          }
          return best_insert_pos_in_style_anchors(anchors, base)
        }
        uni = uni + n
        ent = ent + n
      }
      SeqContent::TextStyleAnchor(_a) => ent = ent + 1
      SeqContent::Unknown(n) => ent = ent + n
      SeqContent::MoveAnchor => ent = ent + 1
      SeqContent::List(_items) =>
        raise LoroError("text index: unexpected list content in text container")
    }
    i = i + 1
  }

  // unicode_pos is validated, so this should be unreachable.
  raise LoroError("text index: cannot resolve entity index")
}

fn LoroDoc::text_entity_ranges_for_delete(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  unicode_pos : Int,
  unicode_len : Int,
) -> Array[(Int, Int)] raise LoroError {
  if unicode_len <= 0 {
    return []
  }
  if unicode_pos < 0 {
    raise LoroError("text.delete: negative index")
  }
  let end = unicode_pos + unicode_len
  let total = self.text_unicode_len(cid)
  if end > total {
    raise LoroError("text.delete: range out of bounds")
  }

  let t = self.ensure_text_tracker(cid)
  let ranges : Array[(Int, Int)] = []
  let mut uni = 0
  let mut ent = 0
  for s in t.rope.spans {
    if !s.status.is_activated() {
      continue
    }
    match s.content {
      SeqContent::Text(_str, n) => {
        let span_start = uni
        let span_end = uni + n
        if span_end <= unicode_pos || span_start >= end {
          uni = span_end
          ent = ent + n
          continue
        }
        let off_start = if unicode_pos > span_start { unicode_pos - span_start } else { 0 }
        let off_end = if end < span_end { end - span_start } else { n }
        let seg_len = off_end - off_start
        if seg_len > 0 {
          let seg_entity_start = ent + off_start
          if ranges.length() == 0 {
            ranges.push((seg_entity_start, seg_len))
          } else {
            let (last_start, last_len) = ranges[ranges.length() - 1]
            if last_start + last_len == seg_entity_start {
              ranges[ranges.length() - 1] = (last_start, last_len + seg_len)
            } else {
              ranges.push((seg_entity_start, seg_len))
            }
          }
        }
        uni = span_end
        ent = ent + n
      }
      SeqContent::TextStyleAnchor(_a) => ent = ent + 1
      SeqContent::Unknown(n) => ent = ent + n
      SeqContent::MoveAnchor => ent = ent + 1
      SeqContent::List(_items) =>
        raise LoroError("text.delete: unexpected list content in text container")
    }
  }
  ranges
}

fn LoroDoc::text_len(self : LoroDoc, cid : @loro_codec.ContainerID) -> Int {
  self.text_unicode_len(cid)
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
  let key = container_id_string(cid)
  let n = utf8_count_chars(text)
  if n == 0 {
    return
  }

  let t = self.ensure_text_tracker(cid)
  let entity_pos = self.text_entity_index_for_insert(cid, index)

  self.start_txn_if_needed()
  let peer = self.inner.peer
  let counter = self.inner.txn_next_counter
  let lamport = self.inner.txn_next_lamport
  t.insert(
    SeqIdFull::new(peer, counter, lamport),
    entity_pos,
    SeqContent::Text(text, n),
  )
  self.inner.text_trackers.set(key, t)

  self.inner.pending_ops.push({
    container: cid,
    counter,
    lamport,
    len: n.reinterpret_as_uint(),
    content: PendingOpContent::TextInsert(entity_pos, text),
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
  let key = container_id_string(cid)
  let ranges = self.text_entity_ranges_for_delete(cid, index, len)
  if ranges.length() == 0 {
    return
  }
  let t = self.ensure_text_tracker(cid)

  self.start_txn_if_needed()
  let peer = self.inner.peer

  // Apply from right to left so entity indexes remain valid.
  for i in 0..<ranges.length() {
    let (entity_pos, entity_len) = ranges[ranges.length() - 1 - i]
    let start_id = t.rope.id_at_active_index(entity_pos)
    let counter = self.inner.txn_next_counter
    let lamport = self.inner.txn_next_lamport
    t.delete(@loro_codec.ID::new(peer, counter), start_id, entity_pos, entity_len, false)
    self.inner.pending_ops.push({
      container: cid,
      counter,
      lamport,
      len: entity_len.reinterpret_as_uint(),
      content: PendingOpContent::TextDelete(entity_pos, entity_len, start_id),
    })
    self.inner.txn_next_counter = counter + entity_len
    self.inner.txn_next_lamport = lamport + entity_len.reinterpret_as_uint()
  }
  self.inner.text_trackers.set(key, t)
}

fn LoroDoc::text_mark_inner(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  start : Int,
  end : Int,
  key : String,
  value : Value,
  is_delete : Bool,
) -> Unit raise LoroError {
  let cid_key = container_id_string(cid)
  if start < 0 || end < start {
    raise LoroError("text.mark: invalid range")
  }
  let text_len = self.text_unicode_len(cid)
  if end > text_len {
    raise LoroError("text.mark: range out of bounds")
  }

  let entity_start = self.text_entity_index_for_insert(cid, start)
  let entity_end = self.text_entity_index_for_insert(cid, end)
  if entity_end < entity_start {
    raise LoroError("text.mark: invalid entity range")
  }

  let info = self.get_text_style_flag(key, is_delete)

  let t = self.ensure_text_tracker(cid)
  self.start_txn_if_needed()
  let peer = self.inner.peer
  let counter = self.inner.txn_next_counter
  let lamport = self.inner.txn_next_lamport
  let style_id = @loro_codec.IdLp::new(peer, lamport)
  t.insert(
    SeqIdFull::new(peer, counter, lamport),
    entity_start,
    SeqContent::TextStyleAnchor({
      style_id,
      key,
      value,
      info,
      anchor_type: TextAnchorType::Start,
    }),
  )
  self.inner.pending_ops.push({
    container: cid,
    counter,
    lamport,
    len: 1U,
    content: PendingOpContent::TextMark(entity_start, entity_end, key, value, info),
  })

  let counter2 = counter + 1
  let lamport2 = lamport + 1U
  t.insert(
    SeqIdFull::new(peer, counter2, lamport2),
    entity_end + 1,
    SeqContent::TextStyleAnchor({
      style_id,
      key,
      value,
      info,
      anchor_type: TextAnchorType::End,
    }),
  )
  self.inner.text_trackers.set(cid_key, t)
  self.inner.pending_ops.push({
    container: cid,
    counter: counter2,
    lamport: lamport2,
    len: 1U,
    content: PendingOpContent::TextMarkEnd,
  })

  self.inner.txn_next_counter = counter2 + 1
  self.inner.txn_next_lamport = lamport2 + 1U
}

fn LoroDoc::text_mark(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  start : Int,
  end : Int,
  key : String,
  value : Value,
) -> Unit raise LoroError {
  self.text_mark_inner(cid, start, end, key, value, false)
}

fn LoroDoc::text_unmark(
  self : LoroDoc,
  cid : @loro_codec.ContainerID,
  start : Int,
  end : Int,
  key : String,
) -> Unit raise LoroError {
  self.text_mark_inner(cid, start, end, key, Value::Null, true)
}

priv struct DeltaRun {
  mut insert : String
  attrs : Array[(String, Value)]
}

fn compute_current_text_attrs(
  active : @hashmap.HashMap[String, Array[TextStyleAnchor]],
) -> Array[(String, Value)] {
  let out : Array[(String, Value)] = []
  for pair in active {
    let (key, set) = pair
    if set.length() == 0 {
      continue
    }
    let mut best = set[0]
    for i in 1..<set.length() {
      let cand = set[i]
      if idlp_gt(cand.style_id, best.style_id) {
        best = cand
      }
    }
    if !value_is_null(best.value) {
      out.push((key, best.value))
    }
  }
  out.sort_by_key(pair => {
    let (k, _v) = pair
    k
  })
  out
}

fn push_text_delta_run(
  runs : Array[DeltaRun],
  insert : String,
  active : @hashmap.HashMap[String, Array[TextStyleAnchor]],
) -> Unit {
  if insert.length() == 0 {
    return
  }
  let attrs = compute_current_text_attrs(active)
  if runs.length() > 0 && runs[runs.length() - 1].attrs == attrs {
    runs[runs.length() - 1].insert = runs[runs.length() - 1].insert + insert
  } else {
    runs.push({ insert, attrs })
  }
}

fn LoroDoc::text_to_delta(self : LoroDoc, cid : @loro_codec.ContainerID) -> Json {
  let t = self.ensure_text_tracker(cid)
  let active : @hashmap.HashMap[String, Array[TextStyleAnchor]] = @hashmap.new(capacity=16)
  let mut buf = ""
  let runs : Array[DeltaRun] = []

  for s in t.rope.spans {
    if !s.status.is_activated() {
      continue
    }
    match s.content {
      SeqContent::Text(str, _n) => buf = buf + str
      SeqContent::TextStyleAnchor(a) => {
        push_text_delta_run(runs, buf, active)
        buf = ""
        let k = a.key
        match a.anchor_type {
          TextAnchorType::Start =>
            match active.get(k) {
              None => {
                let arr : Array[TextStyleAnchor] = []
                arr.push(a)
                active.set(k, arr)
              }
              Some(arr) => arr.push(a)
            }
          TextAnchorType::End =>
            match active.get(k) {
              None => ()
              Some(arr) => {
                let new_arr : Array[TextStyleAnchor] = []
                for x in arr {
                  if x.style_id != a.style_id {
                    new_arr.push(x)
                  }
                }
                if new_arr.length() == 0 {
                  active.remove(k)
                } else {
                  active.set(k, new_arr)
                }
              }
            }
        }
      }
      _ => ()
    }
  }
  push_text_delta_run(runs, buf, active)
  buf = ""

  let out : Array[Json] = []
  for r in runs {
    let obj = Map::new(capacity=2)
    obj["insert"] = Json::string(r.insert)
    if r.attrs.length() > 0 {
      let attrs = Map::new(capacity=r.attrs.length())
      for pair in r.attrs {
        let (k, v) = pair
        attrs[k] = value_to_json_shallow(v)
      }
      obj["attributes"] = Json::object(attrs)
    }
    out.push(Json::object(obj))
  }
  Json::array(out)
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

///|
pub fn LoroText::mark(
  self : LoroText,
  start : Int,
  end : Int,
  key : String,
  value : Value,
) -> Unit raise LoroError {
  self.doc.text_mark(self.cid, start, end, key, value)
}

///|
pub fn LoroText::unmark(self : LoroText, start : Int, end : Int, key : String) -> Unit raise LoroError {
  self.doc.text_unmark(self.cid, start, end, key)
}

///|
pub fn LoroText::toDelta(self : LoroText) -> Json {
  self.doc.text_to_delta(self.cid)
}
