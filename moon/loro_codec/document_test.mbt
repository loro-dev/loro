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

test "encode_document roundtrip" {
  let mode: UInt = 4
  let body: Bytes = b"\x00\x01\x02"
  let encoded = try! encode_document(mode, body)
  let parsed = try! parse_document(encoded, true)
  assert_eq(parsed.mode(), mode)
  assert_eq(parsed.body(), body)
}

fn make_minimal_change_block_bytes() -> Bytes {
  let peer0 = 0x0102030405060708UL

  let header_w = BytesWriter::new()
  header_w.write_uleb128_u64(1UL) // peer_num
  header_w.write_u64_le(peer0)
  header_w.write_bytes(encode_bool_rle([false])) // dep_on_self
  header_w.write_bytes(encode_any_rle_usize([0UL])) // dep lens
  header_w.write_bytes(encode_delta_of_delta_i64([])) // dep counters (empty)
  header_w.write_bytes(encode_delta_of_delta_i64([])) // lamports (empty)
  let header_bytes = header_w.to_bytes()

  let meta_w = BytesWriter::new()
  meta_w.write_bytes(encode_delta_of_delta_i64([1234L])) // timestamp
  meta_w.write_bytes(encode_any_rle_u32([0])) // no commit msg
  let meta_bytes = meta_w.to_bytes()

  let keys_w = BytesWriter::new()
  let root = @encoding/utf8.encode("root"[:])
  keys_w.write_uleb128_u64(root.length().to_uint64())
  keys_w.write_bytes(root)
  let k = @encoding/utf8.encode("a"[:])
  keys_w.write_uleb128_u64(k.length().to_uint64())
  keys_w.write_bytes(k)
  let keys_bytes = keys_w.to_bytes()

  let arena_w = BytesWriter::new()
  arena_w.write_varint_u64(1UL)
  arena_w.write_varint_u64(4UL) // field count
  arena_w.write_u8(b'\x01') // is_root
  arena_w.write_u8(container_type_to_u8(ContainerType::map()))
  arena_w.write_varint_u64(0UL)
  arena_w.write_varint_i64(0L)
  let cids_bytes = arena_w.to_bytes()

  let ops_col0 = encode_delta_rle_u32([0]) // container_index
  let ops_col1 = encode_delta_rle_i32([1]) // prop = key idx
  let ops_col2 = encode_rle_u8([11]) // value_type = LoroValue
  let ops_col3 = encode_rle_u32([1]) // len = 1
  let ops_bytes = encode_columnar_vec_wrapped([ops_col0, ops_col1, ops_col2, ops_col3])

  let values_bytes = b"\x03\x0A"

  let w = BytesWriter::new()
  w.write_varint_u64(10UL) // counter_start
  w.write_varint_u64(1UL) // counter_len
  w.write_varint_u64(100UL) // lamport_start
  w.write_varint_u64(1UL) // lamport_len
  w.write_varint_u64(1UL) // n_changes
  for part in [header_bytes, meta_bytes, cids_bytes, keys_bytes, b"", ops_bytes, b"", values_bytes] {
    w.write_varint_u64(part.length().to_uint64())
    w.write_bytes(part)
  }
  w.to_bytes()
}

test "transcode_document preserves fastsnapshot" {
  let body = encode_fast_snapshot_body(b"op!", b"E", b"")
  let doc = try! encode_document(3, body)
  let out = try! transcode_document(doc, true)
  assert_eq(out, doc)
}

test "transcode_document validates fastupdates blocks" {
  let block = make_minimal_change_block_bytes()
  let body = encode_fast_updates_body([block])
  let doc = try! encode_document(4, body)
  let out = try! transcode_document(doc, true)
  assert_eq(out, doc)
}
