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

test "parse fastsnapshot body" {
  let w = BytesWriter::new()
  w.write_u32_le(3)
  w.write_bytes(b"op!")
  w.write_u32_le(1)
  w.write_bytes(b"E")
  w.write_u32_le(0)

  let parsed = try! parse_fast_snapshot_body(w.to_bytes()[:])
  assert_eq(parsed.oplog_bytes_view().to_bytes(), b"op!")
  assert_eq(parsed.state_bytes_view().to_bytes(), b"E")
  assert_eq(parsed.shallow_root_state_bytes_view().length(), 0)
}

test "parse fastupdates body" {
  let w = BytesWriter::new()
  w.write_uleb128_u64(3)
  w.write_bytes(b"abc")
  w.write_uleb128_u64(1)
  w.write_bytes(b"Z")

  let blocks = try! parse_fast_updates_body(w.to_bytes()[:])
  assert_eq(blocks.length(), 2)
  assert_eq(blocks[0].to_bytes(), b"abc")
  assert_eq(blocks[1].to_bytes(), b"Z")
}
