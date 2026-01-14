test "bytes reader/writer roundtrip" {
  let w = BytesWriter::new()
  w.write_u8(0xAB)
  w.write_u16_le(0x1234)
  w.write_u16_be(0xABCD)
  w.write_u32_le(0x89ABCDEF)
  w.write_u32_be(0x01020304)
  w.write_u64_le(0x0123456789ABCDEF)
  w.write_u64_be(0x0123456789ABCDEF)

  let bytes = w.to_bytes()
  let r = BytesReader::new(bytes)
  assert_eq(try! r.read_u8(), 0xAB)
  assert_eq(try! r.read_u16_le(), 0x1234)
  assert_eq(try! r.read_u16_be(), 0xABCD)
  assert_eq(try! r.read_u32_le(), 0x89ABCDEF)
  assert_eq(try! r.read_u32_be(), 0x01020304)
  assert_eq(try! r.read_u64_le(), 0x0123456789ABCDEF)
  assert_eq(try! r.read_u64_be(), 0x0123456789ABCDEF)
  assert_eq(r.remaining(), 0)
}

test "bytes reader eof" {
  let r = BytesReader::new(b"")
  let b = r.read_u8() catch { DecodeError(_) => b'\xFF' }
  assert_eq(b, b'\xFF')
}
