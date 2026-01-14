test "postcard zigzag i64 examples" {
  fn enc(v: Int64) -> Bytes {
    let w = BytesWriter::new()
    w.write_varint_i64(v)
    w.to_bytes()
  }

  // 重要：zigzag 与 SLEB128 不同
  assert_eq(enc(0), b"\x00")
  assert_eq(enc(-1), b"\x01")
  assert_eq(enc(1), b"\x02")
  assert_eq(enc(-2), b"\x03")
  assert_eq(enc(2), b"\x04")
  assert_eq(enc(-64), b"\x7F")
}

test "postcard varint roundtrip" {
  fn rt(v: UInt64) raise {
    let w = BytesWriter::new()
    w.write_varint_u64(v)
    let bytes = w.to_bytes()
    let r = BytesReader::new(bytes)
    assert_eq(try! r.read_varint_u64(), v)
    assert_eq(r.remaining(), 0)
  }

  try! rt(0)
  try! rt(1)
  try! rt(127)
  try! rt(128)
  try! rt(300)
  try! rt(16384)
  try! rt(0xFFFF_FFFF)
  try! rt(0xFFFF_FFFF_FFFF_FFFF)
}
