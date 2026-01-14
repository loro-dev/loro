const POSTCARD_MAX_COLLECTION_SIZE: UInt64 = 268435456UL

pub enum CommonValue {
  Null
  Bool(Bool)
  Double(Double)
  I64(Int64)
  String(String)
  List(Array[CommonValue])
  Map(Array[(String, CommonValue)])
  Container(ContainerID)
  Binary(Bytes)
} derive(Eq, Show)

// NOTE: MoonBit makes enum constructors read-only across packages. Expose explicit
// constructor functions for blackbox tests and future callers.
pub fn CommonValue::null() -> CommonValue {
  CommonValue::Null
}

pub fn CommonValue::bool(v: Bool) -> CommonValue {
  CommonValue::Bool(v)
}

pub fn CommonValue::double(v: Double) -> CommonValue {
  CommonValue::Double(v)
}

pub fn CommonValue::i64(v: Int64) -> CommonValue {
  CommonValue::I64(v)
}

pub fn CommonValue::string(v: String) -> CommonValue {
  CommonValue::String(v)
}

pub fn CommonValue::binary(v: Bytes) -> CommonValue {
  CommonValue::Binary(v)
}

pub fn CommonValue::list(v: Array[CommonValue]) -> CommonValue {
  CommonValue::List(v)
}

pub fn CommonValue::map(v: Array[(String, CommonValue)]) -> CommonValue {
  CommonValue::Map(v)
}

pub fn CommonValue::container(v: ContainerID) -> CommonValue {
  CommonValue::Container(v)
}

fn read_postcard_len_u64(r: BytesReader) -> UInt64 raise DecodeError {
  let n = r.read_varint_u64()
  if n > POSTCARD_MAX_COLLECTION_SIZE {
    raise DecodeError("postcard: collection too large")
  }
  n
}

fn read_postcard_utf8(r: BytesReader) -> String raise DecodeError {
  let len_u64 = r.read_varint_u64()
  if len_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("postcard: string too long")
  }
  let len = len_u64.to_int()
  if len < 0 || len > r.remaining() {
    raise DecodeError("postcard: invalid string length")
  }
  let bytes = r.read_exact(len)
  @encoding/utf8.decode(bytes) catch { @encoding/utf8.Malformed(_) =>
    raise DecodeError("postcard: invalid utf8")
  }
}

fn write_postcard_utf8(w: BytesWriter, s: String) -> Unit {
  let bytes = @encoding/utf8.encode(s[:])
  w.write_varint_u64(bytes.length().to_uint64())
  w.write_bytes(bytes)
}

fn read_postcard_bytes_val(r: BytesReader) -> Bytes raise DecodeError {
  let len_u64 = r.read_varint_u64()
  if len_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("postcard: bytes too long")
  }
  let len = len_u64.to_int()
  if len < 0 || len > r.remaining() {
    raise DecodeError("postcard: invalid bytes length")
  }
  r.read_exact(len).to_bytes()
}

fn write_postcard_bytes_val(w: BytesWriter, b: Bytes) -> Unit {
  w.write_varint_u64(b.length().to_uint64())
  w.write_bytes(b)
}

pub fn postcard_take_id(bytes: BytesView) -> (ID, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let peer = r.read_varint_u64()
  let counter_i64 = r.read_varint_i64()
  if counter_i64 < -2147483648L || counter_i64 > 2147483647L {
    raise DecodeError("postcard: id counter overflow")
  }
  let id = ID::new(peer, counter_i64.to_int())
  (id, r.remaining_view())
}

pub fn postcard_encode_id(id: ID) -> Bytes {
  let w = BytesWriter::new()
  w.write_varint_u64(id.peer())
  w.write_varint_i64(id.counter().to_int64())
  w.to_bytes()
}

pub fn postcard_take_container_id(bytes: BytesView) -> (ContainerID, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let cid_tag = r.read_varint_u64()
  match cid_tag {
    0UL => {
      let name = read_postcard_utf8(r)
      let kind = container_type_from_historical_u8(r.read_u8())
      (ContainerID::root(name, kind), r.remaining_view())
    }
    1UL => {
      let peer = r.read_varint_u64()
      let counter_i64 = r.read_varint_i64()
      if counter_i64 < -2147483648L || counter_i64 > 2147483647L {
        raise DecodeError("postcard: container counter overflow")
      }
      let kind = container_type_from_historical_u8(r.read_u8())
      (ContainerID::normal(peer, counter_i64.to_int(), kind), r.remaining_view())
    }
    _ => raise DecodeError("postcard: invalid ContainerID tag")
  }
}

pub fn postcard_encode_container_id(cid: ContainerID) -> Bytes {
  let w = BytesWriter::new()
  match cid {
    ContainerID::Root(name, kind) => {
      w.write_varint_u64(0)
      write_postcard_utf8(w, name)
      w.write_u8(container_type_to_historical_u8(kind))
    }
    ContainerID::Normal(peer, counter, kind) => {
      w.write_varint_u64(1)
      w.write_varint_u64(peer)
      w.write_varint_i64(counter.to_int64())
      w.write_u8(container_type_to_historical_u8(kind))
    }
  }
  w.to_bytes()
}

fn read_common_value(r: BytesReader, depth: Int) -> CommonValue raise DecodeError {
  if depth > 1024 {
    raise DecodeError("postcard: value too deep")
  }

  let discr = r.read_varint_u64()
  match discr {
    0UL => CommonValue::Null
    1UL => {
      match r.read_u8() {
        b'\x00' => CommonValue::Bool(false)
        b'\x01' => CommonValue::Bool(true)
        _ => raise DecodeError("postcard: invalid bool")
      }
    }
    2UL => {
      // f64 little-endian
      let bits = r.read_u64_le()
      CommonValue::Double(bits.reinterpret_as_double())
    }
    3UL => {
      // i64 zigzag varint
      CommonValue::I64(r.read_varint_i64())
    }
    4UL => CommonValue::String(read_postcard_utf8(r))
    5UL => {
      let len = read_postcard_len_u64(r).to_int()
      let items: Array[CommonValue] = []
      for _i in 0..<len {
        items.push(read_common_value(r, depth + 1))
      }
      CommonValue::List(items)
    }
    6UL => {
      let len = read_postcard_len_u64(r).to_int()
      let items: Array[(String, CommonValue)] = []
      for _i in 0..<len {
        let k = read_postcard_utf8(r)
        let v = read_common_value(r, depth + 1)
        items.push((k, v))
      }
      CommonValue::Map(items)
    }
    7UL => {
      let (cid, rest) = postcard_take_container_id(r.remaining_view())
      r.skip(r.remaining() - rest.length())
      CommonValue::Container(cid)
    }
    8UL => CommonValue::Binary(read_postcard_bytes_val(r))
    _ => raise DecodeError("postcard: invalid LoroValue discriminant")
  }
}

fn write_common_value(w: BytesWriter, v: CommonValue) -> Unit {
  match v {
    CommonValue::Null => w.write_varint_u64(0)
    CommonValue::Bool(b) => {
      w.write_varint_u64(1)
      w.write_u8(if b { b'\x01' } else { b'\x00' })
    }
    CommonValue::Double(d) => {
      w.write_varint_u64(2)
      w.write_u64_le(d.reinterpret_as_uint64())
    }
    CommonValue::I64(x) => {
      w.write_varint_u64(3)
      w.write_varint_i64(x)
    }
    CommonValue::String(s) => {
      w.write_varint_u64(4)
      write_postcard_utf8(w, s)
    }
    CommonValue::List(items) => {
      w.write_varint_u64(5)
      w.write_varint_u64(items.length().to_uint64())
      for it in items {
        write_common_value(w, it)
      }
    }
    CommonValue::Map(items) => {
      w.write_varint_u64(6)
      w.write_varint_u64(items.length().to_uint64())
      for pair in items {
        let (k, it) = pair
        write_postcard_utf8(w, k)
        write_common_value(w, it)
      }
    }
    CommonValue::Container(cid) => {
      w.write_varint_u64(7)
      w.write_bytes(postcard_encode_container_id(cid))
    }
    CommonValue::Binary(b) => {
      w.write_varint_u64(8)
      write_postcard_bytes_val(w, b)
    }
  }
}

pub fn postcard_take_common_value(bytes: BytesView) -> (CommonValue, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let v = read_common_value(r, 0)
  (v, r.remaining_view())
}

pub fn postcard_encode_common_value(v: CommonValue) -> Bytes {
  let w = BytesWriter::new()
  write_common_value(w, v)
  w.to_bytes()
}

pub fn postcard_take_vec_common_value(bytes: BytesView) -> (Array[CommonValue], BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let len = read_postcard_len_u64(r).to_int()
  let out: Array[CommonValue] = []
  for _i in 0..<len {
    out.push(read_common_value(r, 0))
  }
  (out, r.remaining_view())
}

pub fn postcard_encode_vec_common_value(values: Array[CommonValue]) -> Bytes {
  let w = BytesWriter::new()
  w.write_varint_u64(values.length().to_uint64())
  for v in values {
    write_common_value(w, v)
  }
  w.to_bytes()
}

pub fn postcard_take_vec_string(bytes: BytesView) -> (Array[String], BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let len = read_postcard_len_u64(r).to_int()
  let out: Array[String] = []
  for _i in 0..<len {
    out.push(read_postcard_utf8(r))
  }
  (out, r.remaining_view())
}

pub fn postcard_encode_vec_string(values: Array[String]) -> Bytes {
  let w = BytesWriter::new()
  w.write_varint_u64(values.length().to_uint64())
  for s in values {
    write_postcard_utf8(w, s)
  }
  w.to_bytes()
}

// --- VersionVector / Frontiers ---

pub type VersionVector = Array[(UInt64, Int)] // (peer, counter)

pub fn postcard_take_version_vector(bytes: BytesView) -> (VersionVector, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let len_u64 = r.read_varint_u64()
  if len_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("postcard: vv too large")
  }
  let len = len_u64.to_int()
  let out: VersionVector = []
  for _i in 0..<len {
    let peer = r.read_varint_u64()
    let counter_i64 = r.read_varint_i64()
    if counter_i64 < -2147483648L || counter_i64 > 2147483647L {
      raise DecodeError("postcard: vv counter overflow")
    }
    out.push((peer, counter_i64.to_int()))
  }
  (out, r.remaining_view())
}

pub fn postcard_encode_version_vector(vv: VersionVector) -> Bytes {
  let w = BytesWriter::new()
  w.write_varint_u64(vv.length().to_uint64())
  for pair in vv {
    let (peer, counter) = pair
    w.write_varint_u64(peer)
    w.write_varint_i64(counter.to_int64())
  }
  w.to_bytes()
}

pub fn postcard_take_frontiers(bytes: BytesView) -> (Array[ID], BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let len_u64 = r.read_varint_u64()
  if len_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("postcard: frontiers too large")
  }
  let len = len_u64.to_int()
  let out: Array[ID] = []
  for _i in 0..<len {
    let peer = r.read_varint_u64()
    let counter_i64 = r.read_varint_i64()
    if counter_i64 < -2147483648L || counter_i64 > 2147483647L {
      raise DecodeError("postcard: frontier counter overflow")
    }
    out.push(ID::new(peer, counter_i64.to_int()))
  }
  (out, r.remaining_view())
}

pub fn postcard_encode_frontiers(ids: Array[ID]) -> Bytes {
  let w = BytesWriter::new()
  w.write_varint_u64(ids.length().to_uint64())
  for id in ids {
    w.write_varint_u64(id.peer())
    w.write_varint_i64(id.counter().to_int64())
  }
  w.to_bytes()
}
