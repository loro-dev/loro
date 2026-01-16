///|
fn encode_postcard_vec_usize(values : Array[UInt64]) -> Bytes {
  let w = BytesWriter::new()
  w.write_varint_u64(values.length().to_uint64())
  for v in values {
    w.write_varint_u64(v)
  }
  w.to_bytes()
}

///|
fn decode_postcard_vec_usize(
  bytes : BytesView,
) -> Array[UInt64] raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let n_u64 = r.read_varint_u64()
  if n_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("postcard: usize vec too large")
  }
  let n = n_u64.to_int()
  let out : Array[UInt64] = []
  for _i in 0..<n {
    out.push(r.read_varint_u64())
  }
  if r.remaining() != 0 {
    raise DecodeError("postcard: usize vec trailing bytes")
  }
  out
}

///|
pub fn transcode_tree_state_snapshot(
  bytes : BytesView,
  validate : Bool,
) -> Bytes raise DecodeError {
  let (peers, rest1) = take_peer_table(bytes)
  let r = BytesReader::from_view(rest1)
  let n_fields = r.read_varint_u64()
  if n_fields != 4UL {
    raise DecodeError("tree: invalid EncodedTree field count")
  }

  // node_ids
  let node_ids_view = r.remaining_view()
  let (node_id_cols, rest_after_node_ids) = take_columnar_vec(node_ids_view)
  r.skip(node_ids_view.length() - rest_after_node_ids.length())
  if node_id_cols.length() != 2 {
    raise DecodeError("tree: invalid node_id column count")
  }
  let node_peer_idx = decode_delta_rle_usize(node_id_cols[0])
  let node_counter = decode_delta_rle_i32(node_id_cols[1])
  if node_counter.length() != node_peer_idx.length() {
    raise DecodeError("tree: node_id column length mismatch")
  }

  // nodes
  let nodes_view = r.remaining_view()
  let (node_cols, rest_after_nodes) = take_columnar_vec(nodes_view)
  r.skip(nodes_view.length() - rest_after_nodes.length())
  if node_cols.length() != 5 {
    raise DecodeError("tree: invalid node column count")
  }
  let parent_idx_plus_two = decode_delta_rle_usize(node_cols[0])
  let last_set_peer_idx = decode_delta_rle_usize(node_cols[1])
  let last_set_counter = decode_delta_rle_i32(node_cols[2])
  let last_set_lamport_sub = decode_delta_rle_i32(node_cols[3])
  let fractional_idx_idx = decode_postcard_vec_usize(node_cols[4])
  let n_nodes = node_peer_idx.length()
  if parent_idx_plus_two.length() != n_nodes {
    raise DecodeError("tree: node column length mismatch")
  }
  if last_set_peer_idx.length() != n_nodes {
    raise DecodeError("tree: node column length mismatch")
  }
  if last_set_counter.length() != n_nodes {
    raise DecodeError("tree: node column length mismatch")
  }
  if last_set_lamport_sub.length() != n_nodes {
    raise DecodeError("tree: node column length mismatch")
  }
  if fractional_idx_idx.length() != n_nodes {
    raise DecodeError("tree: node column length mismatch")
  }
  let frac_view = r.remaining_view()
  let (fractional_indexes_bytes, rest_after_frac) = postcard_take_bytes(
    frac_view,
  )
  r.skip(frac_view.length() - rest_after_frac.length())
  let reserved_view = r.remaining_view()
  let (reserved_bytes, rest_after_reserved) = postcard_take_bytes(reserved_view)
  r.skip(reserved_view.length() - rest_after_reserved.length())
  if r.remaining() != 0 {
    raise DecodeError("tree: trailing bytes")
  }
  let positions = decode_position_arena_v2(fractional_indexes_bytes[:])
  if validate {
    for p in node_peer_idx {
      if p > 0x7FFF_FFFFUL || p.to_int() < 0 || p.to_int() >= peers.length() {
        raise DecodeError("tree: node peer_idx out of range")
      }
    }
    for p in last_set_peer_idx {
      if p > 0x7FFF_FFFFUL || p.to_int() < 0 || p.to_int() >= peers.length() {
        raise DecodeError("tree: last_set_peer_idx out of range")
      }
    }
    for i in 0..<n_nodes {
      let parent = parent_idx_plus_two[i]
      if parent > n_nodes.to_uint64() + 1UL {
        raise DecodeError("tree: invalid parent_idx_plus_two")
      }
      let fi = fractional_idx_idx[i]
      if fi > 0x7FFF_FFFFUL ||
        fi.to_int() < 0 ||
        fi.to_int() >= positions.length() {
        raise DecodeError("tree: invalid fractional_index_idx")
      }
    }
  }
  let node_ids_bytes = encode_columnar_vec([
    encode_delta_rle_usize(node_peer_idx),
    encode_delta_rle_i32(node_counter),
  ])
  let nodes_bytes = encode_columnar_vec([
    encode_delta_rle_usize(parent_idx_plus_two),
    encode_delta_rle_usize(last_set_peer_idx),
    encode_delta_rle_i32(last_set_counter),
    encode_delta_rle_i32(last_set_lamport_sub),
    encode_postcard_vec_usize(fractional_idx_idx),
  ])
  let encoded_tree = BytesWriter::new()
  encoded_tree.write_varint_u64(4UL)
  encoded_tree.write_bytes(node_ids_bytes)
  encoded_tree.write_bytes(nodes_bytes)
  // IMPORTANT: TreeState uses Rust `PositionArena::encode()` which is non-empty even when empty.
  encoded_tree.write_bytes(
    postcard_encode_bytes(encode_position_arena(positions)),
  )
  encoded_tree.write_bytes(postcard_encode_bytes(reserved_bytes))
  let w = BytesWriter::new()
  w.write_bytes(encode_peer_table(peers))
  w.write_bytes(encoded_tree.to_bytes())
  w.to_bytes()
}
