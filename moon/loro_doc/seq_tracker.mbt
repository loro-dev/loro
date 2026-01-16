enum SeqOpEffectKind {
  Insert
  Delete(SeqIdSpan)
  Move(@loro_codec.ID)
}

struct SeqOpEffectEntry {
  op_span : SeqIdSpan
  kind : SeqOpEffectKind
}

struct SeqTracker {
  applied_vv : @hashmap.HashMap[UInt64, Int]
  current_vv : @hashmap.HashMap[UInt64, Int]
  rope : SeqRope
  effects : @hashmap.HashMap[UInt64, Array[SeqOpEffectEntry]]
}

fn SeqTracker::new() -> SeqTracker {
  {
    applied_vv: @hashmap.new(capacity=4),
    current_vv: @hashmap.new(capacity=4),
    rope: SeqRope::new(),
    effects: @hashmap.new(capacity=8),
  }
}

fn SeqTracker::new_with_unknown() -> SeqTracker {
  let mut t = SeqTracker::new()
  t.rope.spans.push(
    {
      id: SeqIdFull::new(SEQ_UNKNOWN_PEER_ID, 0, 0U),
      real_id: None,
      status: SeqStatus::new(),
      diff_status: None,
      origin_left: None,
      origin_right: None,
      content: SeqContent::Unknown(0x1FFF_FFFF),
    },
  )
  t
}

fn vv_clone(m : @hashmap.HashMap[UInt64, Int]) -> @hashmap.HashMap[UInt64, Int] {
  let out = @hashmap.new(capacity=8)
  for pair in m {
    let (k, v) = pair
    out.set(k, v)
  }
  out
}

fn vv_diff_iter(
  current : @hashmap.HashMap[UInt64, Int],
  target : @hashmap.HashMap[UInt64, Int],
) -> (Array[SeqIdSpan], Array[SeqIdSpan]) {
  let peers : Array[UInt64] = []
  let seen : @hashmap.HashMap[UInt64, Bool] = @hashmap.new(capacity=16)
  for pair in current {
    let (p, _) = pair
    match seen.get(p) {
      None => {
        seen.set(p, true)
        peers.push(p)
      }
      Some(_) => ()
    }
  }
  for pair in target {
    let (p, _) = pair
    match seen.get(p) {
      None => {
        seen.set(p, true)
        peers.push(p)
      }
      Some(_) => ()
    }
  }
  peers.sort_by_key(p => p)
  let retreat : Array[SeqIdSpan] = []
  let forward : Array[SeqIdSpan] = []
  for p in peers {
    let cur_end = vv_get(current, p)
    let tgt_end = vv_get(target, p)
    if cur_end > tgt_end {
      retreat.push(SeqIdSpan::new(p, tgt_end, cur_end))
    } else if tgt_end > cur_end {
      forward.push(SeqIdSpan::new(p, cur_end, tgt_end))
    }
  }
  (retreat, forward)
}

fn SeqTracker::add_effect(self : SeqTracker, peer : UInt64, entry : SeqOpEffectEntry) -> Unit {
  match self.effects.get(peer) {
    None => {
      let arr : Array[SeqOpEffectEntry] = []
      arr.push(entry)
      self.effects.set(peer, arr)
    }
    Some(arr) => arr.push(entry)
  }
}

fn SeqTracker::checkout(
  self : SeqTracker,
  vv : @hashmap.HashMap[UInt64, Int],
) -> Unit raise LoroError {
  self._checkout(vv, false)
}

fn SeqTracker::_checkout(
  self : SeqTracker,
  vv : @hashmap.HashMap[UInt64, Int],
  on_diff_status : Bool,
) -> Unit raise LoroError {
  if on_diff_status {
    self.rope.clear_diff_status()
  }

  let current = vv_clone(self.current_vv)
  let (retreat, forward) = vv_diff_iter(current, vv)

  for span in retreat {
    self.apply_retract_span(span, on_diff_status)
  }
  for span in forward {
    self.apply_forward_span(span, on_diff_status)
  }

  if !on_diff_status {
    self.current_vv = vv_clone(vv)
  } else {
    self.current_vv = current
  }
}

fn SeqTracker::apply_retract_span(
  self : SeqTracker,
  span : SeqIdSpan,
  on_diff_status : Bool,
) -> Unit raise LoroError {
  let peer = span.peer
  match self.effects.get(peer) {
    None => ()
    Some(effects) =>
      for e in effects {
        let es = e.op_span.start
        let ee = e.op_span.end
        if ee <= span.start || es >= span.end {
          continue
        }
        let os = if es > span.start { es } else { span.start }
        let oe = if ee < span.end { ee } else { span.end }
        if os >= oe {
          continue
        }
        match e.kind {
          SeqOpEffectKind::Insert =>
            self.rope.update_id_range(peer, os, oe, Some(true), 0, on_diff_status)
          SeqOpEffectKind::Delete(target) => {
            let offset = os - es
            let seg_len = oe - os
            self.rope.update_id_range(
              target.peer,
              target.start + offset,
              target.start + offset + seg_len,
              None,
              -1,
              on_diff_status,
            )
          }
          SeqOpEffectKind::Move(from_id) => {
            self.rope.update_id_range(
              from_id.peer(),
              from_id.counter(),
              from_id.counter() + 1,
              None,
              -1,
              on_diff_status,
            )
            self.rope.update_id_range(peer, os, oe, Some(true), 0, on_diff_status)
          }
        }
      }
  }
}

fn SeqTracker::apply_forward_span(
  self : SeqTracker,
  span : SeqIdSpan,
  on_diff_status : Bool,
) -> Unit raise LoroError {
  let peer = span.peer
  match self.effects.get(peer) {
    None => ()
    Some(effects) =>
      for e in effects {
        let es = e.op_span.start
        let ee = e.op_span.end
        if ee <= span.start || es >= span.end {
          continue
        }
        let os = if es > span.start { es } else { span.start }
        let oe = if ee < span.end { ee } else { span.end }
        if os >= oe {
          continue
        }
        match e.kind {
          SeqOpEffectKind::Insert =>
            self.rope.update_id_range(peer, os, oe, Some(false), 0, on_diff_status)
          SeqOpEffectKind::Delete(target) => {
            let offset = os - es
            let seg_len = oe - os
            self.rope.update_id_range(
              target.peer,
              target.start + offset,
              target.start + offset + seg_len,
              None,
              1,
              on_diff_status,
            )
          }
          SeqOpEffectKind::Move(from_id) => {
            self.rope.update_id_range(
              from_id.peer(),
              from_id.counter(),
              from_id.counter() + 1,
              None,
              1,
              on_diff_status,
            )
            self.rope.update_id_range(peer, os, oe, Some(false), 0, on_diff_status)
          }
        }
      }
  }
}

fn SeqTracker::insert(
  self : SeqTracker,
  op_id : SeqIdFull,
  pos : Int,
  content : SeqContent,
) -> Unit raise LoroError {
  let len = seq_content_len(content)
  if len <= 0 {
    raise LoroError("SeqTracker.insert: empty content")
  }
  let peer = op_id.peer
  let end_excl = op_id.counter + len
  if vv_get(self.applied_vv, peer) >= end_excl {
    return
  }

  self.rope.insert_at_active_pos(pos, SeqSpan::new(op_id, content))
  self.add_effect(peer, {
    op_span: SeqIdSpan::new(peer, op_id.counter, end_excl),
    kind: SeqOpEffectKind::Insert,
  })
  vv_set_max(self.current_vv, peer, end_excl)
  vv_set_max(self.applied_vv, peer, end_excl)
}

fn SeqTracker::delete(
  self : SeqTracker,
  op_id_start : @loro_codec.ID,
  target_start_id : @loro_codec.ID,
  pos : Int,
  len : Int,
  reversed : Bool,
) -> Unit raise LoroError {
  if len <= 0 {
    return
  }
  let peer = op_id_start.peer()
  let end_excl = op_id_start.counter() + len
  if vv_get(self.applied_vv, peer) >= end_excl {
    return
  }

  let deleted = self.rope.delete_at_active_pos(target_start_id, pos, len, reversed)
  let mut cur = op_id_start.counter()
  for s in deleted {
    let seg_len = seq_id_span_len(s)
    if seg_len <= 0 {
      continue
    }
    self.add_effect(peer, {
      op_span: SeqIdSpan::new(peer, cur, cur + seg_len),
      kind: SeqOpEffectKind::Delete(s),
    })
    cur = cur + seg_len
  }
  vv_set_max(self.current_vv, peer, end_excl)
  vv_set_max(self.applied_vv, peer, end_excl)
}

fn SeqTracker::move_item(
  self : SeqTracker,
  op_id : SeqIdFull,
  deleted_id : @loro_codec.ID,
  from_pos : Int,
  to_pos : Int,
) -> Unit raise LoroError {
  let peer = op_id.peer
  let end_excl = op_id.counter + 1
  if vv_get(self.applied_vv, peer) >= end_excl {
    return
  }

  let deleted = self.rope.delete_at_active_pos(deleted_id, from_pos, 1, false)
  if deleted.length() == 0 {
    raise LoroError("SeqTracker.move_item: nothing deleted")
  }
  let s = deleted[0]
  let from_id = @loro_codec.ID::new(s.peer, s.start)

  self.rope.insert_at_active_pos(to_pos, {
    id: op_id,
    real_id: if op_id.peer == SEQ_UNKNOWN_PEER_ID { None } else { Some(op_id.id()) },
    status: SeqStatus::new(),
    diff_status: None,
    origin_left: None,
    origin_right: None,
    content: SeqContent::MoveAnchor,
  })
  self.add_effect(peer, {
    op_span: SeqIdSpan::new(peer, op_id.counter, end_excl),
    kind: SeqOpEffectKind::Move(from_id),
  })
  vv_set_max(self.current_vv, peer, end_excl)
  vv_set_max(self.applied_vv, peer, end_excl)
}

fn SeqTracker::diff(
  self : SeqTracker,
  from_vv : @hashmap.HashMap[UInt64, Int],
  to_vv : @hashmap.HashMap[UInt64, Int],
) -> Array[SeqDelta] raise LoroError {
  self._checkout(from_vv, false)
  self._checkout(to_vv, true)
  self.rope.get_diff()
}
