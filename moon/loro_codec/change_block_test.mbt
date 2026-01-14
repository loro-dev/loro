test "change_block: postcard encoded block roundtrip" {
  // u32 fields: 1,2,10,2,1; then 8 bytes fields with varint len.
  let encoded = b"\x01\x02\x0A\x02\x01\x03hdr\x04meta\x01c\x01k\x01p\x01o\x01d\x01v"

  let decoded = try! decode_encoded_block(encoded[:])
  assert_eq(decoded.counter_start(), 1)
  assert_eq(decoded.counter_len(), 2)
  assert_eq(decoded.lamport_start(), 10)
  assert_eq(decoded.lamport_len(), 2)
  assert_eq(decoded.n_changes(), 1)
  assert_eq(decoded.header(), b"hdr")
  assert_eq(decoded.change_meta(), b"meta")
  assert_eq(decoded.cids(), b"c")
  assert_eq(decoded.keys(), b"k")
  assert_eq(decoded.positions(), b"p")
  assert_eq(decoded.ops(), b"o")
  assert_eq(decoded.delete_start_ids(), b"d")
  assert_eq(decoded.values(), b"v")

  let encoded2 = encode_encoded_block(decoded)
  assert_eq(encoded2, encoded)
}

test "change_block: decode header/meta/arenas basics" {
  // --- header ---
  let peers: Array[UInt64] = [0x0102030405060708UL, 0x1112131415161718UL]

  let header_w = BytesWriter::new()
  header_w.write_uleb128_u64(2UL) // peer_num
  for p in peers {
    header_w.write_u64_le(p)
  }
  header_w.write_uleb128_u64(2UL) // atom_len[0]
  header_w.write_uleb128_u64(1UL) // atom_len[1] (last inferred)
  header_w.write_bytes(encode_bool_rle([false, true, false]))
  header_w.write_bytes(encode_any_rle_usize([1UL, 0UL, 2UL]))
  header_w.write_bytes(encode_any_rle_usize([1UL, 1UL, 1UL])) // dep peer idx
  header_w.write_bytes(encode_delta_of_delta_i64([5L, 6L, 7L])) // dep counters
  header_w.write_bytes(encode_delta_of_delta_i64([100L, 105L])) // lamports (n-1)
  let header_bytes = header_w.to_bytes()

  let header = try!
    decode_changes_header(
      header_bytes[:],
      (3).reinterpret_as_uint(),
      (10).reinterpret_as_uint(),
      (7).reinterpret_as_uint(),
      (100).reinterpret_as_uint(),
      (14).reinterpret_as_uint(),
    )

  assert_eq(header.peer(), peers[0])
  assert_eq(header.peers().length(), 2)
  assert_eq(header.peers()[0], peers[0])
  assert_eq(header.peers()[1], peers[1])

  assert_eq(header.lengths().length(), 3)
  assert_eq(header.lengths()[0], 2)
  assert_eq(header.lengths()[1], 1)
  assert_eq(header.lengths()[2], 4)

  assert_eq(header.counters().length(), 4)
  assert_eq(header.counters()[0], 10)
  assert_eq(header.counters()[1], 12)
  assert_eq(header.counters()[2], 13)
  assert_eq(header.counters()[3], 17)

  assert_eq(header.lamports().length(), 3)
  assert_eq(header.lamports()[0], 100)
  assert_eq(header.lamports()[1], 105)
  assert_eq(header.lamports()[2], 110)

  assert_eq(header.deps().length(), 3)
  // change 0 deps: (peer[1], 5)
  assert_eq(header.deps()[0].length(), 1)
  assert_eq(header.deps()[0][0].peer(), peers[1])
  assert_eq(header.deps()[0][0].counter(), 5)
  // change 1 deps: self dep (peer[0], 11)
  assert_eq(header.deps()[1].length(), 1)
  assert_eq(header.deps()[1][0].peer(), peers[0])
  assert_eq(header.deps()[1][0].counter(), 11)
  // change 2 deps: (peer[1], 6), (peer[1], 7)
  assert_eq(header.deps()[2].length(), 2)
  assert_eq(header.deps()[2][0].peer(), peers[1])
  assert_eq(header.deps()[2][0].counter(), 6)
  assert_eq(header.deps()[2][1].peer(), peers[1])
  assert_eq(header.deps()[2][1].counter(), 7)

  // --- meta ---
  let meta_w = BytesWriter::new()
  meta_w.write_bytes(encode_delta_of_delta_i64([1000L, 2000L, 3000L]))
  meta_w.write_bytes(encode_any_rle_u32([0, 2, 0]))
  meta_w.write_bytes(@encoding/utf8.encode("hi"[:]))
  let meta = try! decode_changes_meta(meta_w.to_bytes()[:], (3).reinterpret_as_uint())
  assert_eq(meta.timestamps().length(), 3)
  assert_eq(meta.timestamps()[0], 1000L)
  assert_eq(meta.timestamps()[1], 2000L)
  assert_eq(meta.timestamps()[2], 3000L)
  assert_eq(meta.commit_msgs().length(), 3)
  assert_eq(meta.commit_msgs()[0], None)
  assert_eq(meta.commit_msgs()[1], Some("hi"))
  assert_eq(meta.commit_msgs()[2], None)

  // --- keys ---
  let keys_w = BytesWriter::new()
  let k0 = @encoding/utf8.encode("root"[:])
  keys_w.write_uleb128_u64(k0.length().to_uint64())
  keys_w.write_bytes(k0)
  let k1 = @encoding/utf8.encode("k"[:])
  keys_w.write_uleb128_u64(k1.length().to_uint64())
  keys_w.write_bytes(k1)
  let keys = try! decode_keys(keys_w.to_bytes()[:])
  assert_eq(keys.length(), 2)
  assert_eq(keys[0], "root")
  assert_eq(keys[1], "k")

  // --- container arena ---
  let arena_w = BytesWriter::new()
  arena_w.write_varint_u64(2UL) // vec len
  // root map(name=keys[0])
  arena_w.write_u8(b'\x01')
  arena_w.write_u8(container_type_to_u8(ContainerType::map()))
  arena_w.write_varint_u64(0UL)
  arena_w.write_varint_i64(0L)
  // normal list(peer=peers[1], counter=42)
  arena_w.write_u8(b'\x00')
  arena_w.write_u8(container_type_to_u8(ContainerType::list()))
  arena_w.write_varint_u64(1UL)
  arena_w.write_varint_i64(42L)
  let cids = try! decode_container_arena(arena_w.to_bytes()[:], peers, keys)
  assert_eq(cids.length(), 2)
  assert_eq(cids[0], ContainerID::root("root", ContainerType::map()))
  assert_eq(cids[1], ContainerID::normal(peers[1], 42, ContainerType::list()))
}
