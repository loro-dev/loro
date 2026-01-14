test "parse_document checksum covers mode+body" {
  let mode: UInt = 3
  let body: Bytes = b"\x01\x02\x03"

  let checksum_input = BytesWriter::new()
  checksum_input.write_u16_be(mode)
  checksum_input.write_bytes(body)
  let checksum = try! xxhash32(checksum_input.to_bytes()[:], LORO_XXH32_SEED)

  // Construct: magic(4) + checksum(16) + mode(2) + body
  let w = BytesWriter::new()
  w.write_bytes(b"loro")
  for _i in 0..<12 {
    w.write_u8(0x00)
  }
  // write checksum to bytes[16..20] LE
  w.write_u32_le(checksum)
  w.write_u16_be(mode)
  w.write_bytes(body)

  let doc = try! parse_document(w.to_bytes(), true)
  assert_eq(doc.mode(), mode)
  assert_eq(doc.body(), body)
}

test "parse_document rejects bad checksum" {
  let w = BytesWriter::new()
  w.write_bytes(b"loro")
  for _i in 0..<16 {
    w.write_u8(0x00)
  }
  w.write_u16_be(3)
  w.write_u8(0x01)

  let res = try? parse_document(w.to_bytes(), true)
  match res {
    Ok(_) => assert_eq(true, false)
    Err(_) => ()
  }
}
