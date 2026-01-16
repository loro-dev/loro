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
pub fn postcard_encode_common_value(v : CommonValue) -> Bytes {
  let w = BytesWriter::new()
  write_common_value(w, v)
  w.to_bytes()
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
