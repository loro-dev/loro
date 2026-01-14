test "state_snapshot: map transcode sorts keys" {
  // Build a MapState payload where the visible-map and deleted-keys are intentionally unsorted.
  let values: Array[(String, CommonValue)] = [
    ("b", CommonValue::bool(true)),
    ("a", CommonValue::i64(1L)),
  ]
  let deleted: Array[String] = ["d", "c"]
  let peers: Array[UInt64] = [123UL]

  // Meta is for all keys (a, b, c, d) in sorted order.
  let meta_w = BytesWriter::new()
  for _k in ["a", "b", "c", "d"] {
    meta_w.write_uleb128_u64(0UL) // peer_idx
    meta_w.write_uleb128_u64(7UL) // lamport
  }

  let peer_w = BytesWriter::new()
  peer_w.write_uleb128_u64(peers.length().to_uint64())
  for p in peers {
    peer_w.write_u64_le(p)
  }

  let payload_w = BytesWriter::new()
  payload_w.write_bytes(postcard_encode_map_string_common_value(values))
  payload_w.write_bytes(postcard_encode_vec_string(deleted))
  payload_w.write_bytes(peer_w.to_bytes())
  payload_w.write_bytes(meta_w.to_bytes())
  let payload = payload_w.to_bytes()

  let out = try! transcode_map_state_snapshot(payload[:], true)

  // Expect sorted visible entries and deleted keys in the output.
  let values_sorted: Array[(String, CommonValue)] = [
    ("a", CommonValue::i64(1L)),
    ("b", CommonValue::bool(true)),
  ]
  let deleted_sorted: Array[String] = ["c", "d"]
  let expected_w = BytesWriter::new()
  expected_w.write_bytes(postcard_encode_map_string_common_value(values_sorted))
  expected_w.write_bytes(postcard_encode_vec_string(deleted_sorted))
  expected_w.write_bytes(peer_w.to_bytes())
  expected_w.write_bytes(meta_w.to_bytes())
  let expected = expected_w.to_bytes()
  assert_eq(out, expected)
}

test "state_snapshot: list transcode roundtrip" {
  let values: Array[CommonValue] = [
    CommonValue::i64(1L),
    CommonValue::bool(false),
  ]
  let peers: Array[UInt64] = [1UL]

  let peer_w = BytesWriter::new()
  peer_w.write_uleb128_u64(peers.length().to_uint64())
  for p in peers {
    peer_w.write_u64_le(p)
  }

  let ids_bytes = encode_columnar_vec_wrapped([
    encode_delta_rle_usize([0UL, 0UL]),
    encode_delta_rle_i32([0, 1]),
    encode_delta_rle_i32([0, 0]),
  ])

  let payload_w = BytesWriter::new()
  payload_w.write_bytes(postcard_encode_vec_common_value(values))
  payload_w.write_bytes(peer_w.to_bytes())
  payload_w.write_bytes(ids_bytes)
  let payload = payload_w.to_bytes()

  let out = try! transcode_list_state_snapshot(payload[:], true)
  assert_eq(out, payload)
}

test "state_snapshot: richtext transcode validates unicode scalar length" {
  // "a😀b" has 3 Unicode scalars.
  let text = "a😀b"
  let peers: Array[UInt64] = [7UL]

  let peer_w = BytesWriter::new()
  peer_w.write_uleb128_u64(1UL)
  peer_w.write_u64_le(peers[0])

  // spans: one text span len=3
  let spans_bytes = encode_columnar_vec([
    encode_delta_rle_usize([0UL]),
    encode_delta_rle_i32([0]),
    encode_delta_rle_i32([0]),
    encode_delta_rle_i32([3]),
  ])
  let encoded_text_w = BytesWriter::new()
  encoded_text_w.write_varint_u64(3UL)
  encoded_text_w.write_bytes(spans_bytes)
  encoded_text_w.write_bytes(postcard_encode_vec_string([]))
  encoded_text_w.write_bytes(b"\x00")

  let payload_w = BytesWriter::new()
  payload_w.write_bytes(postcard_encode_string(text))
  payload_w.write_bytes(peer_w.to_bytes())
  payload_w.write_bytes(encoded_text_w.to_bytes())
  let payload = payload_w.to_bytes()

  let out = try! transcode_richtext_state_snapshot(payload[:], true)
  assert_eq(out, payload)
}

test "state_snapshot: tree transcode empty" {
  // Empty tree: no peers, no nodes, empty position arena (must still be encoded as non-empty bytes).
  let peer_w = BytesWriter::new()
  peer_w.write_uleb128_u64(0UL)

  let node_ids_bytes = encode_columnar_vec([encode_delta_rle_usize([]), encode_delta_rle_i32([])])
  let nodes_bytes = encode_columnar_vec([
    encode_delta_rle_usize([]),
    encode_delta_rle_usize([]),
    encode_delta_rle_i32([]),
    encode_delta_rle_i32([]),
    b"\x00",
  ])

  let encoded_tree_w = BytesWriter::new()
  encoded_tree_w.write_varint_u64(4UL)
  encoded_tree_w.write_bytes(node_ids_bytes)
  encoded_tree_w.write_bytes(nodes_bytes)
  encoded_tree_w.write_bytes(postcard_encode_bytes(encode_position_arena([])))
  encoded_tree_w.write_bytes(postcard_encode_bytes(b""))

  let payload_w = BytesWriter::new()
  payload_w.write_bytes(peer_w.to_bytes())
  payload_w.write_bytes(encoded_tree_w.to_bytes())
  let payload = payload_w.to_bytes()

  let out = try! transcode_tree_state_snapshot(payload[:], true)
  assert_eq(out, payload)
}

test "state_snapshot: movable_list transcode minimal" {
  let values: Array[CommonValue] = [CommonValue::i64(1L)]
  let peers: Array[UInt64] = [10UL]

  let peer_w = BytesWriter::new()
  peer_w.write_uleb128_u64(1UL)
  peer_w.write_u64_le(peers[0])

  // items: sentinel + 1 visible
  let items_bytes = encode_columnar_vec([
    encode_delta_rle_usize([0UL, 0UL]),
    encode_bool_rle([true, true]),
    encode_bool_rle([true, true]),
  ])
  // list_item_ids: 1 id for the visible item
  let list_ids_bytes = encode_columnar_vec([
    encode_delta_rle_usize([0UL]),
    encode_delta_rle_i32([0]),
    encode_delta_rle_i32([0]),
  ])
  // elem_ids / last_set_ids: empty
  let elem_ids_bytes = encode_columnar_vec([encode_delta_rle_usize([]), encode_delta_rle_u32([])])
  let last_ids_bytes = encode_columnar_vec([encode_delta_rle_usize([]), encode_delta_rle_u32([])])

  let encoded_w = BytesWriter::new()
  encoded_w.write_varint_u64(4UL)
  encoded_w.write_bytes(items_bytes)
  encoded_w.write_bytes(list_ids_bytes)
  encoded_w.write_bytes(elem_ids_bytes)
  encoded_w.write_bytes(last_ids_bytes)

  let payload_w = BytesWriter::new()
  payload_w.write_bytes(postcard_encode_vec_common_value(values))
  payload_w.write_bytes(peer_w.to_bytes())
  payload_w.write_bytes(encoded_w.to_bytes())
  let payload = payload_w.to_bytes()

  let out = try! transcode_movable_list_state_snapshot(payload[:], true)
  assert_eq(out, payload)
}

test "state_snapshot: counter transcode" {
  let payload = b"\x00\x00\x00\x00\x00\x00\xF0\x3F" // 1.0 (f64 LE)
  let out = try! transcode_counter_state_snapshot(payload[:], true)
  assert_eq(out, payload)
}
