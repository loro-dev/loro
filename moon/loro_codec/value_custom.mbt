const MAX_COLLECTION_SIZE: UInt64 = 268435456UL

pub enum LoroValue {
  Null
  True
  False
  I64(Int64)
  F64(Double)
  Str(String)
  Binary(Bytes)
  List(Array[LoroValue])
  Map(Array[(UInt64, LoroValue)]) // (key_idx, value)
  ContainerType(Byte)
} derive(Eq, Show)

pub enum Value {
  Null
  True
  False
  I64(Int64)
  F64(Double)
  Str(String)
  Binary(Bytes)
  ContainerIdx(UInt64)
  DeleteOnce
  DeleteSeq
  DeltaInt(Int)
  LoroValue(LoroValue)
  MarkStart(MarkStart)
  TreeMove(TreeMove)
  ListMove(ListMove)
  ListSet(ListSet)
  RawTreeMove(RawTreeMove)
  Future(Byte, Bytes) // (raw tag byte, binary payload bytes)
} derive(Eq, Show)

pub struct MarkStart {
  info: Byte
  len: UInt64
  key_idx: UInt64
  value: LoroValue
} derive(Eq, Show)

pub struct TreeMove {
  target_idx: UInt64
  is_parent_null: Bool
  position: UInt64
  parent_idx: UInt64?
} derive(Eq, Show)

pub struct ListMove {
  from: UInt64
  from_idx: UInt64
  lamport: UInt64
} derive(Eq, Show)

pub struct ListSet {
  peer_idx: UInt64
  lamport: UInt
  value: LoroValue
} derive(Eq, Show)

pub struct RawTreeMove {
  subject_peer_idx: UInt64
  subject_cnt: Int
  position_idx: UInt64
  is_parent_null: Bool
  parent_peer_idx: UInt64
  parent_cnt: Int
} derive(Eq, Show)

fn read_len_u64(r: BytesReader) -> UInt64 raise DecodeError {
  let n = r.read_uleb128_u64()
  if n > MAX_COLLECTION_SIZE {
    raise DecodeError("value: collection too large")
  }
  n
}

fn read_utf8(r: BytesReader) -> String raise DecodeError {
  let len_u64 = r.read_uleb128_u64()
  if len_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("value: string too long")
  }
  let len = len_u64.to_int()
  if len < 0 || len > r.remaining() {
    raise DecodeError("value: invalid string length")
  }
  let bytes = r.read_exact(len)
  @encoding/utf8.decode(bytes) catch { @encoding/utf8.Malformed(_) =>
    raise DecodeError("value: invalid utf8")
  }
}

fn read_binary(r: BytesReader) -> Bytes raise DecodeError {
  let len_u64 = r.read_uleb128_u64()
  if len_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("value: binary too long")
  }
  let len = len_u64.to_int()
  if len < 0 || len > r.remaining() {
    raise DecodeError("value: invalid binary length")
  }
  r.read_exact(len).to_bytes()
}

fn write_utf8(w: BytesWriter, s: String) -> Unit {
  let bytes = @encoding/utf8.encode(s[:])
  w.write_uleb128_u64(bytes.length().to_uint64())
  w.write_bytes(bytes)
}

fn write_binary(w: BytesWriter, b: Bytes) -> Unit {
  w.write_uleb128_u64(b.length().to_uint64())
  w.write_bytes(b)
}

fn read_loro_value(r: BytesReader, depth: Int) -> LoroValue raise DecodeError {
  if depth > 1024 {
    raise DecodeError("value: too deep")
  }
  let kind = r.read_u8().to_uint()
  match kind {
    0 => LoroValue::Null
    1 => LoroValue::True
    2 => LoroValue::False
    3 => LoroValue::I64(r.read_sleb128_i64())
    4 => {
      let bits = r.read_u64_be()
      LoroValue::F64(bits.reinterpret_as_double())
    }
    5 => LoroValue::Str(read_utf8(r))
    6 => LoroValue::Binary(read_binary(r))
    7 => {
      let len = read_len_u64(r).to_int()
      let items: Array[LoroValue] = []
      for _i in 0..<len {
        items.push(read_loro_value(r, depth + 1))
      }
      LoroValue::List(items)
    }
    8 => {
      let len = read_len_u64(r).to_int()
      let items: Array[(UInt64, LoroValue)] = []
      for _i in 0..<len {
        let key_idx = r.read_uleb128_u64()
        let v = read_loro_value(r, depth + 1)
        items.push((key_idx, v))
      }
      LoroValue::Map(items)
    }
    9 => {
      let ct = r.read_u8()
      LoroValue::ContainerType(ct)
    }
    _ => raise DecodeError("value: invalid loro_value kind")
  }
}

fn write_loro_value(w: BytesWriter, v: LoroValue) -> Unit {
  match v {
    LoroValue::Null => w.write_u8(b'\x00')
    LoroValue::True => w.write_u8(b'\x01')
    LoroValue::False => w.write_u8(b'\x02')
    LoroValue::I64(x) => {
      w.write_u8(b'\x03')
      w.write_sleb128_i64(x)
    }
    LoroValue::F64(x) => {
      w.write_u8(b'\x04')
      w.write_u64_be(x.reinterpret_as_uint64())
    }
    LoroValue::Str(s) => {
      w.write_u8(b'\x05')
      write_utf8(w, s)
    }
    LoroValue::Binary(b) => {
      w.write_u8(b'\x06')
      write_binary(w, b)
    }
    LoroValue::List(items) => {
      w.write_u8(b'\x07')
      w.write_uleb128_u64(items.length().to_uint64())
      for it in items {
        write_loro_value(w, it)
      }
    }
    LoroValue::Map(items) => {
      w.write_u8(b'\x08')
      w.write_uleb128_u64(items.length().to_uint64())
      for pair in items {
        let (k, it) = pair
        w.write_uleb128_u64(k)
        write_loro_value(w, it)
      }
    }
    LoroValue::ContainerType(ct) => {
      w.write_u8(b'\x09')
      w.write_u8(ct)
    }
  }
}

pub fn decode_value(bytes: BytesView) -> (Value, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let tag = r.read_u8()
  decode_value_content(tag, bytes[1:])
}

pub fn decode_value_content(tag: Byte, bytes: BytesView) -> (Value, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let kind = (tag.to_uint() & 0x7F)
  let v: Value =
    match kind {
      0 => Value::Null
      1 => Value::True
      2 => Value::False
      3 => Value::I64(r.read_sleb128_i64())
      4 => {
        let bits = r.read_u64_be()
        Value::F64(bits.reinterpret_as_double())
      }
      5 => Value::Str(read_utf8(r))
      6 => Value::Binary(read_binary(r))
      7 => Value::ContainerIdx(r.read_uleb128_u64())
      8 => Value::DeleteOnce
      9 => Value::DeleteSeq
      10 => {
        let x = r.read_sleb128_i64()
        if x < -2147483648L || x > 2147483647L {
          raise DecodeError("value: deltaint overflow")
        }
        Value::DeltaInt(x.to_int())
      }
      11 => Value::LoroValue(read_loro_value(r, 0))
      12 => {
        let info = r.read_u8()
        let len = r.read_uleb128_u64()
        let key_idx = r.read_uleb128_u64()
        let value = read_loro_value(r, 0)
        Value::MarkStart({ info, len, key_idx, value })
      }
      13 => {
        let target_idx = r.read_uleb128_u64()
        let is_parent_null = r.read_u8() != b'\x00'
        let position = r.read_uleb128_u64()
        let parent_idx =
          if is_parent_null {
            Option::None
          } else {
            Option::Some(r.read_uleb128_u64())
          }
        Value::TreeMove({ target_idx, is_parent_null, position, parent_idx })
      }
      14 => {
        let from = r.read_uleb128_u64()
        let from_idx = r.read_uleb128_u64()
        let lamport = r.read_uleb128_u64()
        Value::ListMove({ from, from_idx, lamport })
      }
      15 => {
        let peer_idx = r.read_uleb128_u64()
        let lamport_u64 = r.read_uleb128_u64()
        if lamport_u64 > 0xFFFF_FFFFUL {
          raise DecodeError("value: listset lamport overflow")
        }
        let value = read_loro_value(r, 0)
        Value::ListSet({ peer_idx, lamport: lamport_u64.to_uint(), value })
      }
      16 => {
        let subject_peer_idx = r.read_uleb128_u64()
        let subject_cnt_u64 = r.read_uleb128_u64()
        let position_idx = r.read_uleb128_u64()
        let is_parent_null = r.read_u8() != b'\x00'
        let mut parent_peer_idx: UInt64 = 0
        let mut parent_cnt_u64: UInt64 = 0
        if !is_parent_null {
          parent_peer_idx = r.read_uleb128_u64()
          parent_cnt_u64 = r.read_uleb128_u64()
        }
        if subject_cnt_u64 > 0x7FFF_FFFFUL || parent_cnt_u64 > 0x7FFF_FFFFUL {
          raise DecodeError("value: raw_tree_move counter overflow")
        }
        Value::RawTreeMove({
          subject_peer_idx,
          subject_cnt: subject_cnt_u64.to_int(),
          position_idx,
          is_parent_null,
          parent_peer_idx,
          parent_cnt: parent_cnt_u64.to_int(),
        })
      }
      _ => {
        let data = read_binary(r)
        Value::Future(tag, data)
      }
    }

  let rest = bytes[bytes.length() - r.remaining():]
  (v, rest)
}

pub fn encode_value_content(v: Value) -> (Byte, Bytes) {
  let w = BytesWriter::new()
  match v {
    Value::Null => (b'\x00', b"")
    Value::True => (b'\x01', b"")
    Value::False => (b'\x02', b"")
    Value::I64(x) => {
      w.write_sleb128_i64(x)
      (b'\x03', w.to_bytes())
    }
    Value::F64(x) => {
      w.write_u64_be(x.reinterpret_as_uint64())
      (b'\x04', w.to_bytes())
    }
    Value::Str(s) => {
      write_utf8(w, s)
      (b'\x05', w.to_bytes())
    }
    Value::Binary(b) => {
      write_binary(w, b)
      (b'\x06', w.to_bytes())
    }
    Value::ContainerIdx(idx) => {
      w.write_uleb128_u64(idx)
      (b'\x07', w.to_bytes())
    }
    Value::DeleteOnce => (b'\x08', b"")
    Value::DeleteSeq => (b'\x09', b"")
    Value::DeltaInt(x) => {
      w.write_sleb128_i64(x.to_int64())
      (b'\x0A', w.to_bytes())
    }
    Value::LoroValue(v) => {
      write_loro_value(w, v)
      (b'\x0B', w.to_bytes())
    }
    Value::MarkStart(m) => {
      w.write_u8(m.info)
      w.write_uleb128_u64(m.len)
      w.write_uleb128_u64(m.key_idx)
      write_loro_value(w, m.value)
      (b'\x0C', w.to_bytes())
    }
    Value::TreeMove(t) => {
      w.write_uleb128_u64(t.target_idx)
      w.write_u8(if t.is_parent_null { b'\x01' } else { b'\x00' })
      w.write_uleb128_u64(t.position)
      match t.parent_idx {
        Option::None => ()
        Option::Some(p) => w.write_uleb128_u64(p)
      }
      (b'\x0D', w.to_bytes())
    }
    Value::ListMove(m) => {
      w.write_uleb128_u64(m.from)
      w.write_uleb128_u64(m.from_idx)
      w.write_uleb128_u64(m.lamport)
      (b'\x0E', w.to_bytes())
    }
    Value::ListSet(s) => {
      w.write_uleb128_u64(s.peer_idx)
      w.write_uleb128_u64(s.lamport.to_uint64())
      write_loro_value(w, s.value)
      (b'\x0F', w.to_bytes())
    }
    Value::RawTreeMove(t) => {
      w.write_uleb128_u64(t.subject_peer_idx)
      w.write_uleb128_u64(t.subject_cnt.to_uint64())
      w.write_uleb128_u64(t.position_idx)
      w.write_u8(if t.is_parent_null { b'\x01' } else { b'\x00' })
      if !t.is_parent_null {
        w.write_uleb128_u64(t.parent_peer_idx)
        w.write_uleb128_u64(t.parent_cnt.to_uint64())
      }
      (b'\x10', w.to_bytes())
    }
    Value::Future(tag, data) => {
      write_binary(w, data)
      (tag, w.to_bytes())
    }
  }
}

pub fn encode_value(v: Value) -> Bytes {
  let w = BytesWriter::new()
  let (tag, content) = encode_value_content(v)
  w.write_u8(tag)
  w.write_bytes(content)
  w.to_bytes()
}
