///|
pub fn postcard_take_bytes(
  bytes : BytesView,
) -> (Bytes, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let b = read_postcard_bytes_val(r)
  (b, r.remaining_view())
}

///|
pub fn postcard_encode_bytes(b : Bytes) -> Bytes {
  let w = BytesWriter::new()
  write_postcard_bytes_val(w, b)
  w.to_bytes()
}
