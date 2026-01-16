///|
pub fn postcard_take_vec_string(
  bytes : BytesView,
) -> (Array[String], BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let len = read_postcard_len_u64(r).to_int()
  let out : Array[String] = []
  for _i in 0..<len {
    out.push(read_postcard_utf8(r))
  }
  (out, r.remaining_view())
}

///|
pub fn postcard_encode_vec_string(values : Array[String]) -> Bytes {
  let w = BytesWriter::new()
  w.write_varint_u64(values.length().to_uint64())
  for s in values {
    write_postcard_utf8(w, s)
  }
  w.to_bytes()
}
