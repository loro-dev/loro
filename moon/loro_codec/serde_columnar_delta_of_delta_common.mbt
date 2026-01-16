///|
fn read_postcard_option_i64(r : BytesReader) -> Int64? raise DecodeError {
  let tag = r.read_varint_u64()
  match tag {
    0UL => None
    1UL => Some(r.read_varint_i64())
    _ => raise DecodeError("postcard: invalid Option tag")
  }
}

///|
fn write_postcard_option_i64(w : BytesWriter, v : Int64?) -> Unit {
  match v {
    None => w.write_varint_u64(0)
    Some(x) => {
      w.write_varint_u64(1)
      w.write_varint_i64(x)
    }
  }
}
