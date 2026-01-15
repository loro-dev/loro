///|
pub fn postcard_take_string(
  bytes : BytesView,
) -> (String, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let s = read_postcard_utf8(r)
  (s, r.remaining_view())
}

///|
pub fn postcard_encode_string(s : String) -> Bytes {
  let w = BytesWriter::new()
  write_postcard_utf8(w, s)
  w.to_bytes()
}
