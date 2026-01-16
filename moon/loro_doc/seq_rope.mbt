enum SeqDiffStatus {
  NotChanged
  Created
  Deleted
}

struct SeqSpan {
  id : SeqIdFull
  real_id : @loro_codec.ID?
  status : SeqStatus
  diff_status : SeqStatus?
  origin_left : @loro_codec.ID?
  origin_right : @loro_codec.ID?
  content : SeqContent
}

fn SeqSpan::new(id : SeqIdFull, content : SeqContent) -> SeqSpan {
  let real_id = if id.peer == SEQ_UNKNOWN_PEER_ID {
    None
  } else {
    Some(id.id())
  }
  {
    id,
    real_id,
    status: SeqStatus::new(),
    diff_status: None,
    origin_left: None,
    origin_right: None,
    content,
  }
}

fn SeqSpan::rle_len(self : SeqSpan) -> Int {
  seq_content_len(self.content)
}

fn SeqSpan::id_span(self : SeqSpan) -> SeqIdSpan {
  SeqIdSpan::new(self.id.peer, self.id.counter, self.id.counter + self.rle_len())
}

fn SeqSpan::is_activated(self : SeqSpan) -> Bool {
  self.status.is_activated()
}

fn SeqSpan::is_activated_in_diff(self : SeqSpan) -> Bool {
  match self.diff_status {
    None => self.status.is_activated()
    Some(s) => s.is_activated()
  }
}

fn SeqSpan::diff(self : SeqSpan) -> SeqDiffStatus {
  match self.diff_status {
    None => SeqDiffStatus::NotChanged
    Some(d) => {
      let old_active = self.status.is_activated()
      let new_active = d.is_activated()
      if old_active && !new_active {
        SeqDiffStatus::Deleted
      } else if !old_active && new_active {
        SeqDiffStatus::Created
      } else {
        SeqDiffStatus::NotChanged
      }
    }
  }
}

fn option_id_eq(a : @loro_codec.ID?, b : @loro_codec.ID?) -> Bool {
  match a {
    None => match b {
      None => true
      Some(_) => false
    }
    Some(x) => match b {
      None => false
      Some(y) => x == y
    }
  }
}

fn option_status_eq(a : SeqStatus?, b : SeqStatus?) -> Bool {
  match a {
    None => match b {
      None => true
      Some(_) => false
    }
    Some(x) => match b {
      None => false
      Some(y) => seq_status_eq(x, y)
    }
  }
}

fn SeqSpan::can_merge(self : SeqSpan, rhs : SeqSpan) -> Bool raise LoroError {
  let len = self.rle_len()
  if self.id.peer != rhs.id.peer {
    return false
  }
  if !seq_status_eq(self.status, rhs.status) || !option_status_eq(self.diff_status, rhs.diff_status) {
    return false
  }
  if self.id.counter + len != rhs.id.counter {
    return false
  }
  if self.id.lamport + len.reinterpret_as_uint() != rhs.id.lamport {
    return false
  }
  let last_id = self.id.inc(len - 1).id()
  match rhs.origin_left {
    None => return false
    Some(ol) => if ol != last_id {
      return false
    }
  }
  if !option_id_eq(self.origin_right, rhs.origin_right) {
    return false
  }
  if !seq_content_can_merge(self.content, rhs.content) {
    return false
  }
  match (self.real_id, rhs.real_id) {
    (None, None) => true
    (Some(a), Some(b)) => id_inc(a, len) == b
    _ => false
  }
}

fn SeqSpan::merge_right(self : SeqSpan, rhs : SeqSpan) -> Unit raise LoroError {
  self.content = seq_content_merge_right(self.content, rhs.content)
}

fn SeqSpan::slice(self : SeqSpan, start : Int, end : Int) -> SeqSpan raise LoroError {
  let len = self.rle_len()
  if start < 0 || end < start || end > len {
    raise LoroError("SeqSpan.slice: invalid range")
  }
  let new_id = self.id.inc(start)
  let new_real_id = match self.real_id {
    None => None
    Some(rid) => Some(id_inc(rid, start))
  }
  let new_origin_left = if start == 0 {
    self.origin_left
  } else {
    Some(self.id.inc(start - 1).id())
  }
  {
    id: new_id,
    real_id: new_real_id,
    status: self.status,
    diff_status: self.diff_status,
    origin_left: new_origin_left,
    origin_right: self.origin_right,
    content: seq_content_slice(self.content, start, end),
  }
}

struct SeqCursor {
  idx : Int
  offset : Int
}

fn make_spans_with_insert(
  spans : Array[SeqSpan],
  index : Int,
  value : SeqSpan,
) -> Array[SeqSpan] {
  let out : Array[SeqSpan] = []
  for i in 0..<index {
    out.push(spans[i])
  }
  out.push(value)
  for i in index..<spans.length() {
    out.push(spans[i])
  }
  out
}

fn make_spans_with_replace_split(
  spans : Array[SeqSpan],
  index : Int,
  left : SeqSpan,
  right : SeqSpan,
) -> Array[SeqSpan] {
  let out : Array[SeqSpan] = []
  for i in 0..<index {
    out.push(spans[i])
  }
  out.push(left)
  out.push(right)
  for i in (index + 1)..<spans.length() {
    out.push(spans[i])
  }
  out
}

fn make_spans_with_replace_three(
  spans : Array[SeqSpan],
  index : Int,
  a : SeqSpan,
  b : SeqSpan,
  c : SeqSpan,
) -> Array[SeqSpan] {
  let out : Array[SeqSpan] = []
  for i in 0..<index {
    out.push(spans[i])
  }
  out.push(a)
  out.push(b)
  out.push(c)
  for i in (index + 1)..<spans.length() {
    out.push(spans[i])
  }
  out
}

struct SeqInsertScanElem {
  idx : Int
  id : SeqIdFull
  origin_left : @loro_codec.ID?
  origin_right : @loro_codec.ID?
  len : Int
}

struct SeqRope {
  spans : Array[SeqSpan]
}

fn SeqRope::new() -> SeqRope {
  { spans: [] }
}

fn SeqRope::active_len(self : SeqRope) -> Int {
  let mut sum = 0
  for s in self.spans {
    if s.status.is_activated() {
      sum = sum + s.rle_len()
    }
  }
  sum
}

fn SeqRope::id_at_active_index(self : SeqRope, pos : Int) -> @loro_codec.ID raise LoroError {
  let c = self.find_active_cursor_prefer_right(pos)
  if c.idx < 0 || c.idx >= self.spans.length() {
    raise LoroError("SeqRope.id_at_active_index: out of range")
  }
  let s = self.spans[c.idx]
  if !s.status.is_activated() {
    raise LoroError("SeqRope.id_at_active_index: not activated")
  }
  s.id.inc(c.offset).id()
}

fn SeqRope::clear_diff_status(self : SeqRope) -> Unit {
  for i in 0..<self.spans.length() {
    self.spans[i].diff_status = None
  }
}

fn SeqRope::find_span_index_starting_at(
  self : SeqRope,
  id : @loro_codec.ID,
) -> Int? {
  for i in 0..<self.spans.length() {
    if self.spans[i].id.id() == id {
      return Some(i)
    }
  }
  None
}

fn SeqRope::cmp_pos(self : SeqRope, a : Int?, b : Int?) -> Int {
  match (a, b) {
    (Some(x), Some(y)) =>
      if x < y {
        -1
      } else if x > y {
        1
      } else {
        0
      }
    (Some(_), None) => -1
    (None, Some(_)) => 1
    (None, None) => 0
  }
}

fn SeqRope::find_active_cursor_prefer_left(
  self : SeqRope,
  pos : Int,
) -> SeqCursor raise LoroError {
  if pos < 0 {
    raise LoroError("SeqRope.find_active_cursor_prefer_left: negative pos")
  }
  let mut left = pos
  for i in 0..<self.spans.length() {
    let s = self.spans[i]
    if s.status.is_activated() {
      let l = s.rle_len()
      if left <= l {
        return { idx: i, offset: left }
      }
      left = left - l
    } else {
      if left == 0 {
        return { idx: i, offset: 0 }
      }
    }
  }
  if left == 0 {
    { idx: self.spans.length(), offset: 0 }
  } else {
    raise LoroError("SeqRope.find_active_cursor_prefer_left: pos out of range")
  }
}

fn SeqRope::find_active_cursor_prefer_right(
  self : SeqRope,
  pos : Int,
) -> SeqCursor raise LoroError {
  if pos < 0 {
    raise LoroError("SeqRope.find_active_cursor_prefer_right: negative pos")
  }
  let mut left = pos
  for i in 0..<self.spans.length() {
    let s = self.spans[i]
    if s.status.is_activated() {
      let l = s.rle_len()
      if left < l {
        return { idx: i, offset: left }
      }
      left = left - l
    } else {
      // skip zero-len spans
      ()
    }
  }
  if left == 0 {
    { idx: self.spans.length(), offset: 0 }
  } else {
    raise LoroError("SeqRope.find_active_cursor_prefer_right: pos out of range")
  }
}

fn SeqRope::merge_pass(self : SeqRope) -> Unit raise LoroError {
  if self.spans.length() == 0 {
    return
  }
  let out : Array[SeqSpan] = []
  out.push(self.spans[0])
  for i in 1..<self.spans.length() {
    let cur = self.spans[i]
    let last_idx = out.length() - 1
    let last = out[last_idx]
    if last.can_merge(cur) {
      out[last_idx].merge_right(cur)
    } else {
      out.push(cur)
    }
  }
  self.spans = out
}

fn SeqRope::insert_by_cursor(
  self : SeqRope,
  cursor : SeqCursor,
  content : SeqSpan,
) -> Unit raise LoroError {
  if cursor.idx < 0 || cursor.offset < 0 {
    raise LoroError("SeqRope.insert_by_cursor: invalid cursor")
  }
  if cursor.idx == self.spans.length() {
    if cursor.offset != 0 {
      raise LoroError("SeqRope.insert_by_cursor: invalid end cursor")
    }
    self.spans.push(content)
    self.merge_pass()
    return
  }
  if cursor.idx > self.spans.length() {
    raise LoroError("SeqRope.insert_by_cursor: cursor idx out of range")
  }
  let target = self.spans[cursor.idx]
  let len = target.rle_len()
  if cursor.offset > len {
    raise LoroError("SeqRope.insert_by_cursor: cursor offset out of range")
  }
  if cursor.offset == 0 {
    self.spans = make_spans_with_insert(self.spans, cursor.idx, content)
    self.merge_pass()
    return
  }
  if cursor.offset == len {
    self.spans = make_spans_with_insert(self.spans, cursor.idx + 1, content)
    self.merge_pass()
    return
  }
  let left = target.slice(0, cursor.offset)
  let right = target.slice(cursor.offset, len)
  self.spans = make_spans_with_replace_three(self.spans, cursor.idx, left, content, right)
  self.merge_pass()
}

fn SeqRope::insert_at_active_pos(
  self : SeqRope,
  pos : Int,
  content : SeqSpan,
) -> Unit raise LoroError {
  if self.spans.length() == 0 {
    if pos != 0 {
      raise LoroError("SeqRope.insert: non-zero pos on empty rope")
    }
    self.spans.push(content)
    return
  }

  let start = self.find_active_cursor_prefer_left(pos)
  let origin_left = if start.offset == 0 {
    if start.idx == 0 {
      None
    } else {
      let left_span = self.spans[start.idx - 1]
      Some(left_span.id.inc(left_span.rle_len() - 1).id())
    }
  } else {
    let left_span = self.spans[start.idx]
    Some(left_span.id.inc(start.offset - 1).id())
  }

  let mut origin_right : @loro_codec.ID? = None
  let mut parent_right_leaf : Int? = None
  let in_between : Array[SeqInsertScanElem] = []

  for i in start.idx..<self.spans.length() {
    let elem = self.spans[i]
    let start_off = if i == start.idx {
      start.offset
    } else {
      0
    }
    if start_off >= elem.rle_len() {
      continue
    }
    if !elem.status.future {
      origin_right = Some(elem.id.inc(start_off).id())
      if start_off > 0 {
        parent_right_leaf = Some(i)
      } else {
        if option_id_eq(elem.origin_left, origin_left) {
          parent_right_leaf = Some(i)
        } else {
          parent_right_leaf = None
        }
      }
      break
    }
    in_between.push({
      idx: i,
      id: elem.id,
      origin_left: elem.origin_left,
      origin_right: elem.origin_right,
      len: elem.rle_len(),
    })
  }

  let mut new_span = content
  new_span.origin_left = origin_left
  new_span.origin_right = origin_right

  let mut insert_pos = start
  if in_between.length() != 0 {
    let mut scanning = false
    let visited : Array[SeqIdSpan] = []
    for other in in_between {
      let other_origin_left = other.origin_left
      if !option_id_eq(other_origin_left, new_span.origin_left) {
        let should_break = match other_origin_left {
          None => true
          Some(left_id) => {
            let mut all_not_contains = true
            for v in visited {
              if v.contains_id(left_id) {
                all_not_contains = false
                break
              }
            }
            all_not_contains
          }
        }
        if should_break {
          break
        }
      }

      visited.push(
        SeqIdSpan::new(
          other.id.peer,
          other.id.counter,
          other.id.counter + other.len,
        ),
      )

      if option_id_eq(new_span.origin_left, other_origin_left) {
        if option_id_eq(other.origin_right, new_span.origin_right) {
          if other.id.peer > new_span.id.peer {
            break
          } else {
            scanning = false
          }
        } else {
          let other_parent_right = match other.origin_right {
            None => None
            Some(orid) =>
              match self.find_span_index_starting_at(orid) {
                None => None
                Some(idx) =>
                  if option_id_eq(self.spans[idx].origin_left, new_span.origin_left) {
                    Some(idx)
                  } else {
                    None
                  }
              }
          }
          let cmp = self.cmp_pos(other_parent_right, parent_right_leaf)
          if cmp < 0 {
            scanning = true
          } else if cmp == 0 && other.id.peer > new_span.id.peer {
            break
          } else {
            scanning = false
          }
        }
      }

      if !scanning {
        insert_pos = { idx: other.idx, offset: other.len }
      }
    }
  }

  self.insert_by_cursor(insert_pos, new_span)
}

fn SeqRope::delete_at_active_pos(
  self : SeqRope,
  start_id : @loro_codec.ID,
  pos : Int,
  len : Int,
  reversed : Bool,
) -> Array[SeqIdSpan] raise LoroError {
  if len <= 0 {
    return []
  }
  if reversed && len > 1 {
    let out : Array[SeqIdSpan] = []
    for i in 0..<len {
      let j = len - 1 - i
      let sub = self.delete_at_active_pos(id_inc(start_id, j), pos + j, 1, false)
      for s in sub {
        out.push(s)
      }
    }
    return out
  }

  let start = self.find_active_cursor_prefer_right(pos)
  let end = self.find_active_cursor_prefer_left(pos + len)

  let mut end_idx = end.idx
  let mut end_off = end.offset
  if end_idx < self.spans.length() {
    let end_len = self.spans[end_idx].rle_len()
    if end_off == end_len {
      end_idx = end_idx + 1
      end_off = 0
    } else if end_off > 0 {
      let e = self.spans[end_idx]
      let left = e.slice(0, end_off)
      let right = e.slice(end_off, end_len)
      self.spans = make_spans_with_replace_split(self.spans, end_idx, left, right)
      end_idx = end_idx + 1
      end_off = 0
    }
  }

  let mut start_idx = start.idx
  let mut start_off = start.offset
  if start_idx >= self.spans.length() {
    raise LoroError("SeqRope.delete: start cursor out of range")
  }
  let start_len = self.spans[start_idx].rle_len()
  if start_off == start_len {
    start_idx = start_idx + 1
    start_off = 0
  } else if start_off > 0 {
    let s = self.spans[start_idx]
    let left = s.slice(0, start_off)
    let right = s.slice(start_off, start_len)
    self.spans = make_spans_with_replace_split(self.spans, start_idx, left, right)
    start_idx = start_idx + 1
    start_off = 0
    if end_idx > start.idx {
      end_idx = end_idx + 1
    }
  }

  let out : Array[SeqIdSpan] = []
  let mut cur_start_id = start_id
  for i in start_idx..<end_idx {
    if i < 0 || i >= self.spans.length() {
      break
    }
    if self.spans[i].status.is_activated() {
      let span_len = self.spans[i].rle_len()
      out.push(self.spans[i].id_span())
      self.spans[i].status.delete_times = self.spans[i].status.delete_times + 1
      match self.spans[i].real_id {
        None => self.spans[i].real_id = Some(cur_start_id)
        Some(_) => ()
      }
      cur_start_id = id_inc(cur_start_id, span_len)
    }
  }
  self.merge_pass()
  out
}

struct SeqDeltaInsert {
  id : @loro_codec.ID
  lamport : UInt?
  content : SeqContent
}

enum SeqDelta {
  Retain(Int)
  Insert(SeqDeltaInsert)
  Delete(Int)
}

fn SeqRope::get_diff(self : SeqRope) -> Array[SeqDelta] {
  let out : Array[SeqDelta] = []
  let mut last_pos = 0
  let mut index = 0
  for s in self.spans {
    let cur_index = index
    let diff = s.diff()
    match diff {
      SeqDiffStatus::NotChanged => ()
      SeqDiffStatus::Created => {
        if cur_index > last_pos {
          out.push(SeqDelta::Retain(cur_index - last_pos))
          last_pos = cur_index
        }
        let id = match s.real_id {
          None => s.id.id()
          Some(r) => r
        }
        let lamport = if s.id.peer == SEQ_UNKNOWN_PEER_ID {
          None
        } else {
          Some(s.id.lamport)
        }
        out.push(SeqDelta::Insert({ id, lamport, content: s.content }))
      }
      SeqDiffStatus::Deleted => {
        if cur_index > last_pos {
          out.push(SeqDelta::Retain(cur_index - last_pos))
        }
        out.push(SeqDelta::Delete(s.rle_len()))
        last_pos = cur_index + s.rle_len()
      }
    }
    if s.status.is_activated() {
      index = index + s.rle_len()
    }
  }
  out
}

fn SeqRope::update_id_range(
  self : SeqRope,
  peer : UInt64,
  start : Int,
  end : Int,
  set_future : Bool?,
  delete_times_diff : Int,
  on_diff_status : Bool,
) -> Unit raise LoroError {
  if start >= end {
    return
  }
  let out : Array[SeqSpan] = []
  for span0 in self.spans {
    let mut span = span0
    if span.id.peer != peer {
      out.push(span)
      continue
    }
    let s0 = span.id.counter
    let s1 = s0 + span.rle_len()
    if end <= s0 || start >= s1 {
      out.push(span)
      continue
    }
    let ov_start = if start > s0 { start - s0 } else { 0 }
    let ov_end = if end < s1 { end - s0 } else { span.rle_len() }
    if ov_start == 0 && ov_end == span.rle_len() {
      // full
      if on_diff_status {
        let mut d = match span.diff_status {
          None => span.status
          Some(x) => x
        }
        match set_future {
          None => ()
          Some(f) => d.future = f
        }
        d.delete_times = d.delete_times + delete_times_diff
        span.diff_status = Some(d)
      } else {
        match set_future {
          None => ()
          Some(f) => span.status.future = f
        }
        span.status.delete_times = span.status.delete_times + delete_times_diff
      }
      out.push(span)
    } else {
      if ov_start > 0 {
        out.push(span.slice(0, ov_start))
      }
      let mut mid = span.slice(ov_start, ov_end)
      if on_diff_status {
        let mut d = match mid.diff_status {
          None => mid.status
          Some(x) => x
        }
        match set_future {
          None => ()
          Some(f) => d.future = f
        }
        d.delete_times = d.delete_times + delete_times_diff
        mid.diff_status = Some(d)
      } else {
        match set_future {
          None => ()
          Some(f) => mid.status.future = f
        }
        mid.status.delete_times = mid.status.delete_times + delete_times_diff
      }
      out.push(mid)
      if ov_end < span.rle_len() {
        out.push(span.slice(ov_end, span.rle_len()))
      }
    }
  }
  self.spans = out
  self.merge_pass()
}
