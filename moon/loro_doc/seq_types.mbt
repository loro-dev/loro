const SEQ_UNKNOWN_PEER_ID : UInt64 = 0xFFFF_FFFF_FFFF_FFFFUL

struct SeqIdFull {
  peer : UInt64
  counter : Int
  lamport : UInt
}

fn SeqIdFull::new(peer : UInt64, counter : Int, lamport : UInt) -> SeqIdFull {
  { peer, counter, lamport }
}

fn SeqIdFull::id(self : SeqIdFull) -> @loro_codec.ID {
  @loro_codec.ID::new(self.peer, self.counter)
}

fn SeqIdFull::inc(self : SeqIdFull, delta : Int) -> SeqIdFull {
  {
    peer: self.peer,
    counter: self.counter + delta,
    lamport: self.lamport + delta.reinterpret_as_uint(),
  }
}

struct SeqIdSpan {
  peer : UInt64
  start : Int
  end : Int
}

fn SeqIdSpan::new(peer : UInt64, start : Int, end : Int) -> SeqIdSpan {
  { peer, start, end }
}

fn SeqIdSpan::contains_id(self : SeqIdSpan, id : @loro_codec.ID) -> Bool {
  id.peer() == self.peer && id.counter() >= self.start && id.counter() < self.end
}

fn seq_id_span_len(span : SeqIdSpan) -> Int {
  span.end - span.start
}

struct SeqStatus {
  future : Bool
  delete_times : Int
}

fn SeqStatus::new() -> SeqStatus {
  { future: false, delete_times: 0 }
}

fn SeqStatus::is_activated(self : SeqStatus) -> Bool {
  self.delete_times == 0 && !self.future
}

enum SeqContent {
  List(Array[Value])
  Text(String, Int) // (utf8 string, unicode scalar count)
  Unknown(Int)
  MoveAnchor
}

fn seq_status_eq(a : SeqStatus, b : SeqStatus) -> Bool {
  a.future == b.future && a.delete_times == b.delete_times
}

fn utf8_char_len_from_first_byte(first : Byte) -> Int {
  let b = first.to_uint()
  if b < 0x80 {
    1
  } else if (b & 0xE0) == 0xC0 {
    2
  } else if (b & 0xF0) == 0xE0 {
    3
  } else if (b & 0xF8) == 0xF0 {
    4
  } else {
    1
  }
}

fn utf8_count_chars(s : String) -> Int {
  let bytes = @encoding/utf8.encode(s[:])
  let mut i = 0
  let mut n = 0
  while i < bytes.length() {
    let step = utf8_char_len_from_first_byte(bytes[i])
    i = i + step
    n = n + 1
  }
  n
}

fn utf8_byte_index_of_char(s : String, char_index : Int) -> Int raise LoroError {
  if char_index < 0 {
    raise LoroError("utf8: negative char index")
  }
  let bytes = @encoding/utf8.encode(s[:])
  let mut i = 0
  let mut c = 0
  while i < bytes.length() && c < char_index {
    let step = utf8_char_len_from_first_byte(bytes[i])
    i = i + step
    c = c + 1
  }
  if c != char_index {
    raise LoroError("utf8: char index out of range")
  }
  i
}

fn utf8_slice_by_chars(s : String, start : Int, end : Int) -> String raise LoroError {
  if start < 0 || end < start {
    raise LoroError("utf8: invalid slice range")
  }
  let bytes = @encoding/utf8.encode(s[:])
  let start_b = utf8_byte_index_of_char(s, start)
  let end_b = utf8_byte_index_of_char(s, end)
  if start_b < 0 || end_b < start_b || end_b > bytes.length() {
    raise LoroError("utf8: invalid byte range")
  }
  @encoding/utf8.decode(bytes[start_b:end_b]) catch {
    @encoding/utf8.Malformed(_) => raise LoroError("utf8: malformed slice")
  }
}

fn seq_content_len(content : SeqContent) -> Int {
  match content {
    SeqContent::List(items) => items.length()
    SeqContent::Text(_s, n) => n
    SeqContent::Unknown(n) => n
    SeqContent::MoveAnchor => 1
  }
}

fn seq_content_slice(content : SeqContent, start : Int, end : Int) -> SeqContent raise LoroError {
  if start < 0 || end < start {
    raise LoroError("seq_content_slice: invalid range")
  }
  let len = end - start
  match content {
    SeqContent::List(items) => {
      if end > items.length() {
        raise LoroError("seq_content_slice: out of range")
      }
      let out : Array[Value] = []
      for i in start..<end {
        out.push(items[i])
      }
      SeqContent::List(out)
    }
    SeqContent::Text(s, n) => {
      if end > n {
        raise LoroError("seq_content_slice: text out of range")
      }
      let sub = utf8_slice_by_chars(s, start, end)
      SeqContent::Text(sub, len)
    }
    SeqContent::Unknown(n) => {
      if end > n {
        raise LoroError("seq_content_slice: unknown out of range")
      }
      SeqContent::Unknown(len)
    }
    SeqContent::MoveAnchor => {
      if len != 1 {
        raise LoroError("seq_content_slice: move anchor must be len=1")
      }
      SeqContent::MoveAnchor
    }
  }
}

fn seq_content_can_merge(a : SeqContent, b : SeqContent) -> Bool {
  match (a, b) {
    (SeqContent::List(_), SeqContent::List(_)) => true
    (SeqContent::Text(_, _), SeqContent::Text(_, _)) => true
    (SeqContent::Unknown(_), SeqContent::Unknown(_)) => true
    _ => false
  }
}

fn seq_content_merge_right(a : SeqContent, b : SeqContent) -> SeqContent raise LoroError {
  match (a, b) {
    (SeqContent::List(xs), SeqContent::List(ys)) => {
      let out : Array[Value] = []
      for v in xs {
        out.push(v)
      }
      for v in ys {
        out.push(v)
      }
      SeqContent::List(out)
    }
    (SeqContent::Text(s1, n1), SeqContent::Text(s2, n2)) =>
      SeqContent::Text(s1 + s2, n1 + n2)
    (SeqContent::Unknown(n1), SeqContent::Unknown(n2)) => SeqContent::Unknown(n1 + n2)
    _ => raise LoroError("seq_content_merge_right: incompatible kinds")
  }
}
