///|
const POSTCARD_MAX_COLLECTION_SIZE : UInt64 = 268435456UL

///|
fn read_postcard_len_u64(r : BytesReader) -> UInt64 raise DecodeError {
  let n = r.read_varint_u64()
  if n > POSTCARD_MAX_COLLECTION_SIZE {
    raise DecodeError("postcard: collection too large")
  }
  n
}

///|
fn read_postcard_utf8(r : BytesReader) -> String raise DecodeError {
  let len_u64 = r.read_varint_u64()
  if len_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("postcard: string too long")
  }
  let len = len_u64.to_int()
  if len < 0 || len > r.remaining() {
    raise DecodeError("postcard: invalid string length")
  }
  let bytes = r.read_exact(len)
  @encoding/utf8.decode(bytes) catch {
    @encoding/utf8.Malformed(_) => raise DecodeError("postcard: invalid utf8")
  }
}

///|
fn write_postcard_utf8(w : BytesWriter, s : String) -> Unit {
  let bytes = @encoding/utf8.encode(s[:])
  w.write_varint_u64(bytes.length().to_uint64())
  w.write_bytes(bytes)
}

///|
fn read_postcard_bytes_val(r : BytesReader) -> Bytes raise DecodeError {
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

///|
fn write_postcard_bytes_val(w : BytesWriter, b : Bytes) -> Unit {
  w.write_varint_u64(b.length().to_uint64())
  w.write_bytes(b)
}
