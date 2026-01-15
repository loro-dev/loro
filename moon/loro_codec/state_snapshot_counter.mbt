///|
pub fn transcode_counter_state_snapshot(
  bytes : BytesView,
  _validate : Bool,
) -> Bytes raise DecodeError {
  if bytes.length() != 8 {
    raise DecodeError("counter_state: invalid length")
  }
  // f64 little-endian, keep exact bits.
  let bits = BytesReader::from_view(bytes).read_u64_le()
  let w = BytesWriter::new()
  w.write_u64_le(bits)
  w.to_bytes()
}
