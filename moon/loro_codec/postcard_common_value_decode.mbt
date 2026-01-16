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
pub fn postcard_take_common_value(
  bytes : BytesView,
) -> (CommonValue, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let v = read_common_value(r, 0)
  (v, r.remaining_view())
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
