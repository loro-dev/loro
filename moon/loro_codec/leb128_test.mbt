test "uleb128 examples" {
  fn roundtrip(v: UInt64, expected: Bytes) raise {
    let w = BytesWriter::new()
    w.write_uleb128_u64(v)
    assert_eq(w.to_bytes(), expected)
    let r = BytesReader::new(expected)
    assert_eq(try! r.read_uleb128_u64(), v)
    assert_eq(r.remaining(), 0)
  }

  try! roundtrip(0, b"\x00")
  try! roundtrip(1, b"\x01")
  try! roundtrip(127, b"\x7F")
  try! roundtrip(128, b"\x80\x01")
  try! roundtrip(300, b"\xAC\x02")
  try! roundtrip(16384, b"\x80\x80\x01")
}

test "sleb128 examples" {
  fn roundtrip(v: Int64, expected: Bytes) raise {
    let w = BytesWriter::new()
    w.write_sleb128_i64(v)
    assert_eq(w.to_bytes(), expected)
    let r = BytesReader::new(expected)
    assert_eq(try! r.read_sleb128_i64(), v)
    assert_eq(r.remaining(), 0)
  }

  try! roundtrip(0, b"\x00")
  try! roundtrip(1, b"\x01")
  try! roundtrip(-1, b"\x7F")
  try! roundtrip(63, b"\x3F")
  try! roundtrip(-64, b"\x40")
  try! roundtrip(64, b"\xC0\x00")
  try! roundtrip(-65, b"\xBF\x7F")
  try! roundtrip(127, b"\xFF\x00")
  try! roundtrip(-128, b"\x80\x7F")
}
