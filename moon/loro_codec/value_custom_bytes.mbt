///|
fn read_len_u64(r : BytesReader) -> UInt64 raise DecodeError {
  let n = r.read_uleb128_u64()
  if n > MAX_COLLECTION_SIZE {
    raise DecodeError("value: collection too large")
  }
  n
}

///|
fn read_utf8(r : BytesReader) -> String raise DecodeError {
  let len_u64 = r.read_uleb128_u64()
  if len_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("value: string too long")
  }
  let len = len_u64.to_int()
  if len < 0 || len > r.remaining() {
    raise DecodeError("value: invalid string length")
  }
  let bytes = r.read_exact(len)
  @encoding/utf8.decode(bytes) catch {
    @encoding/utf8.Malformed(_) => raise DecodeError("value: invalid utf8")
  }
}

///|
fn read_binary(r : BytesReader) -> Bytes raise DecodeError {
  let len_u64 = r.read_uleb128_u64()
  if len_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("value: binary too long")
  }
  let len = len_u64.to_int()
  if len < 0 || len > r.remaining() {
    raise DecodeError("value: invalid binary length")
  }
  r.read_exact(len).to_bytes()
}

///|
fn write_utf8(w : BytesWriter, s : String) -> Unit {
  let bytes = @encoding/utf8.encode(s[:])
  w.write_uleb128_u64(bytes.length().to_uint64())
  w.write_bytes(bytes)
}

///|
fn write_binary(w : BytesWriter, b : Bytes) -> Unit {
  w.write_uleb128_u64(b.length().to_uint64())
  w.write_bytes(b)
}
