///|
pub fn postcard_take_id(bytes : BytesView) -> (ID, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let peer = r.read_varint_u64()
  let counter_i64 = r.read_varint_i64()
  if counter_i64 < -2147483648L || counter_i64 > 2147483647L {
    raise DecodeError("postcard: id counter overflow")
  }
  let id = ID::new(peer, counter_i64.to_int())
  (id, r.remaining_view())
}

///|
pub fn postcard_encode_id(id : ID) -> Bytes {
  let w = BytesWriter::new()
  w.write_varint_u64(id.peer())
  w.write_varint_i64(id.counter().to_int64())
  w.to_bytes()
}
