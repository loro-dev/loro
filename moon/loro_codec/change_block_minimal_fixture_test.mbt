///|
fn build_minimal_map_insert_encoded_block(peer0 : UInt64) -> Bytes {
  // --- header (n_changes = 1) ---
  let header_w = BytesWriter::new()
  header_w.write_uleb128_u64(1UL) // peer_num
  header_w.write_u64_le(peer0)
  header_w.write_bytes(encode_bool_rle([false])) // dep_on_self
  header_w.write_bytes(encode_any_rle_usize([0UL])) // dep lens
  header_w.write_bytes(encode_delta_of_delta_i64([])) // dep counters (empty)
  header_w.write_bytes(encode_delta_of_delta_i64([])) // lamports (empty, n-1=0)
  let header_bytes = header_w.to_bytes()

  // --- meta (n_changes = 1) ---
  let meta_w = BytesWriter::new()
  meta_w.write_bytes(encode_delta_of_delta_i64([1234L])) // timestamp
  meta_w.write_bytes(encode_any_rle_u32([0])) // no commit msg
  let meta_bytes = meta_w.to_bytes()

  // --- keys ---
  let keys_w = BytesWriter::new()
  let root = @encoding/utf8.encode("root"[:])
  keys_w.write_uleb128_u64(root.length().to_uint64())
  keys_w.write_bytes(root)
  let k = @encoding/utf8.encode("a"[:])
  keys_w.write_uleb128_u64(k.length().to_uint64())
  keys_w.write_bytes(k)
  let keys_bytes = keys_w.to_bytes()

  // --- container arena (1 root map, name=keys[0]) ---
  let arena_w = BytesWriter::new()
  arena_w.write_varint_u64(1UL)
  arena_w.write_varint_u64(4UL) // field count
  arena_w.write_u8(b'\x01') // is_root
  arena_w.write_u8(container_type_to_u8(ContainerType::map()))
  arena_w.write_varint_u64(0UL)
  arena_w.write_varint_i64(0L)
  let cids_bytes = arena_w.to_bytes()

  // --- ops (1 row) ---
  let ops_col0 = encode_delta_rle_u32([0]) // container_index
  let ops_col1 = encode_delta_rle_i32([1]) // prop = key idx
  let ops_col2 = encode_rle_u8([11]) // value_type = LoroValue
  let ops_col3 = encode_rle_u32([1]) // len = 1
  let ops_bytes = encode_columnar_vec_wrapped([
    ops_col0, ops_col1, ops_col2, ops_col3,
  ])

  // --- values: Value::LoroValue(I64(10)) content only (no tag byte) ---
  // LoroValue::I64 => [0x03, sleb128(10)=0x0A]
  let values_bytes = b"\x03\x0A"

  // postcard EncodedBlock
  let w = BytesWriter::new()
  w.write_varint_u64(10UL) // counter_start
  w.write_varint_u64(1UL) // counter_len
  w.write_varint_u64(100UL) // lamport_start
  w.write_varint_u64(1UL) // lamport_len
  w.write_varint_u64(1UL) // n_changes
  for
    part in [
      header_bytes, meta_bytes, cids_bytes, keys_bytes, b"", ops_bytes, b"", values_bytes,
    ] {
    w.write_varint_u64(part.length().to_uint64())
    w.write_bytes(part)
  }
  w.to_bytes()
}

///|
test "change_block: decode full block with ops/values" {
  let peer0 = 0x0102030405060708UL
  let encoded = build_minimal_map_insert_encoded_block(peer0)
  let decoded = try! decode_change_block(encoded[:])
  assert_eq(decoded.length(), 1)
  let c = decoded[0]
  assert_eq(c.id(), ID::new(peer0, 10))
  assert_eq(c.timestamp(), 1234L)
  assert_eq(c.deps().length(), 0)
  assert_eq(c.lamport(), 100)
  assert_eq(c.msg(), None)
  assert_eq(c.ops().length(), 1)
  let op = c.ops()[0]
  assert_eq(op.counter(), 10)
  assert_eq(op.len(), 1)
  assert_eq(op.container(), ContainerID::root("root", ContainerType::map()))
  match op.content() {
    OpContent::Map(MapOp::Insert(key, v)) => {
      assert_eq(key, "a")
      match v {
        LoroValue::I64(x) => assert_eq(x, 10L)
        _ => fail("unexpected map value")
      }
    }
    _ => fail("unexpected op content")
  }
}

///|
test "change_block: decode -> encode -> decode (semantic) roundtrip" {
  // Reuse the minimal block fixture (one change with one Map insert op).
  let peer0 = 0x0102030405060708UL
  let encoded = build_minimal_map_insert_encoded_block(peer0)
  let decoded = try! decode_change_block_full(encoded[:])
  let encoded2 = encode_change_block(decoded) catch {
    EncodeError(msg) => fail(msg)
  }
  let changes1 = try! decode_change_block(encoded[:])
  let changes2 = try! decode_change_block(encoded2[:])
  assert_eq(changes2, changes1)
}
