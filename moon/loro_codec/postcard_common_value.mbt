///|
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

///|
pub fn CommonValue::null() -> CommonValue {
  CommonValue::Null
}

///|
pub fn CommonValue::bool(v : Bool) -> CommonValue {
  CommonValue::Bool(v)
}

///|
pub fn CommonValue::double(v : Double) -> CommonValue {
  CommonValue::Double(v)
}

///|
pub fn CommonValue::i64(v : Int64) -> CommonValue {
  CommonValue::I64(v)
}

///|
pub fn CommonValue::string(v : String) -> CommonValue {
  CommonValue::String(v)
}

///|
pub fn CommonValue::binary(v : Bytes) -> CommonValue {
  CommonValue::Binary(v)
}

///|
pub fn CommonValue::list(v : Array[CommonValue]) -> CommonValue {
  CommonValue::List(v)
}

///|
pub fn CommonValue::map(v : Array[(String, CommonValue)]) -> CommonValue {
  CommonValue::Map(v)
}

///|
pub fn CommonValue::container(v : ContainerID) -> CommonValue {
  CommonValue::Container(v)
}

///|
fn read_common_value(
  r : BytesReader,
  depth : Int,
) -> CommonValue raise DecodeError {
  if depth > 1024 {
    raise DecodeError("postcard: value too deep")
  }
  let discr = r.read_varint_u64()
  match discr {
    0UL => CommonValue::Null
    1UL =>
      match r.read_u8() {
        b'\x00' => CommonValue::Bool(false)
        b'\x01' => CommonValue::Bool(true)
        _ => raise DecodeError("postcard: invalid bool")
      }
    2UL => {
      // f64 little-endian
      let bits = r.read_u64_le()
      CommonValue::Double(bits.reinterpret_as_double())
    }
    3UL =>
      // i64 zigzag varint
      CommonValue::I64(r.read_varint_i64())
    4UL => CommonValue::String(read_postcard_utf8(r))
    5UL => {
      let len = read_postcard_len_u64(r).to_int()
      let items : Array[CommonValue] = []
      for _i in 0..<len {
        items.push(read_common_value(r, depth + 1))
      }
      CommonValue::List(items)
    }
    6UL => {
      let len = read_postcard_len_u64(r).to_int()
      let items : Array[(String, CommonValue)] = []
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

///|
fn write_common_value(w : BytesWriter, v : CommonValue) -> Unit {
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

///|
pub fn postcard_take_common_value(
  bytes : BytesView,
) -> (CommonValue, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let v = read_common_value(r, 0)
  (v, r.remaining_view())
}

///|
pub fn postcard_encode_common_value(v : CommonValue) -> Bytes {
  let w = BytesWriter::new()
  write_common_value(w, v)
  w.to_bytes()
}

///|
pub fn postcard_take_vec_common_value(
  bytes : BytesView,
) -> (Array[CommonValue], BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let len = read_postcard_len_u64(r).to_int()
  let out : Array[CommonValue] = []
  for _i in 0..<len {
    out.push(read_common_value(r, 0))
  }
  (out, r.remaining_view())
}

///|
pub fn postcard_encode_vec_common_value(values : Array[CommonValue]) -> Bytes {
  let w = BytesWriter::new()
  w.write_varint_u64(values.length().to_uint64())
  for v in values {
    write_common_value(w, v)
  }
  w.to_bytes()
}

///|
pub fn postcard_take_map_string_common_value(
  bytes : BytesView,
) -> (Array[(String, CommonValue)], BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let len = read_postcard_len_u64(r).to_int()
  let out : Array[(String, CommonValue)] = []
  for _i in 0..<len {
    let k = read_postcard_utf8(r)
    let v = read_common_value(r, 0)
    out.push((k, v))
  }
  (out, r.remaining_view())
}

///|
pub fn postcard_encode_map_string_common_value(
  values : Array[(String, CommonValue)],
) -> Bytes {
  let w = BytesWriter::new()
  w.write_varint_u64(values.length().to_uint64())
  for pair in values {
    let (k, v) = pair
    write_postcard_utf8(w, k)
    write_common_value(w, v)
  }
  w.to_bytes()
}
