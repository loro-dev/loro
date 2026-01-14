fn take_peer_table(bytes: BytesView) -> (Array[UInt64], BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let n_u64 = r.read_uleb128_u64()
  if n_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("state: too many peers")
  }
  let n = n_u64.to_int()
  let peers: Array[UInt64] = []
  for _i in 0..<n {
    peers.push(r.read_u64_le())
  }
  (peers, r.remaining_view())
}

fn encode_peer_table(peers: Array[UInt64]) -> Bytes {
  let w = BytesWriter::new()
  w.write_uleb128_u64(peers.length().to_uint64())
  for p in peers {
    w.write_u64_le(p)
  }
  w.to_bytes()
}

fn count_utf8_codepoints(s: String) -> Int {
  let bytes = @encoding/utf8.encode(s[:])
  let mut count = 0
  for b in bytes {
    // Count non-continuation bytes: 0b10xxxxxx
    let u = b.to_uint()
    if (u & 0xC0) != 0x80 {
      count = count + 1
    }
  }
  count
}

fn encode_postcard_vec_usize(values: Array[UInt64]) -> Bytes {
  let w = BytesWriter::new()
  w.write_varint_u64(values.length().to_uint64())
  for v in values {
    w.write_varint_u64(v)
  }
  w.to_bytes()
}

fn decode_postcard_vec_usize(bytes: BytesView) -> Array[UInt64] raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let n_u64 = r.read_varint_u64()
  if n_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("postcard: usize vec too large")
  }
  let n = n_u64.to_int()
  let out: Array[UInt64] = []
  for _i in 0..<n {
    out.push(r.read_varint_u64())
  }
  if r.remaining() != 0 {
    raise DecodeError("postcard: usize vec trailing bytes")
  }
  out
}

fn collect_sorted_unique_keys(
  values: Array[(String, CommonValue)],
  deleted_keys: Array[String],
  validate: Bool,
) -> Array[String] raise DecodeError {
  let keys: Array[String] = []
  for pair in values {
    let (k, _v) = pair
    keys.push(k)
  }
  for k in deleted_keys {
    keys.push(k)
  }
  keys.sort()
  if !validate {
    return keys
  }
  // Validate uniqueness.
  if keys.length() <= 1 {
    return keys
  }
  let out: Array[String] = []
  out.push(keys[0])
  for i in 1..<keys.length() {
    if keys[i] == keys[i - 1] {
      raise DecodeError("map_state: duplicate key")
    }
    out.push(keys[i])
  }
  out
}

pub fn transcode_map_state_snapshot(bytes: BytesView, validate: Bool) -> Bytes raise DecodeError {
  let (values, rest1) = postcard_take_map_string_common_value(bytes)
  let (deleted_keys, rest2) = postcard_take_vec_string(rest1)
  let (peers, meta_bytes) = take_peer_table(rest2)

  let keys = collect_sorted_unique_keys(values, deleted_keys, validate)
  let meta_r = BytesReader::from_view(meta_bytes)
  let peer_idxs: Array[UInt64] = []
  let lamports: Array[UInt64] = []
  for _k in keys {
    let peer_idx = meta_r.read_uleb128_u64()
    let lamport = meta_r.read_uleb128_u64()
    if validate {
      if peer_idx > 0x7FFF_FFFFUL {
        raise DecodeError("map_state: peer_idx too large")
      }
      if peer_idx.to_int() < 0 || peer_idx.to_int() >= peers.length() {
        raise DecodeError("map_state: peer_idx out of range")
      }
    }
    peer_idxs.push(peer_idx)
    lamports.push(lamport)
  }
  if meta_r.remaining() != 0 {
    raise DecodeError("map_state: trailing meta bytes")
  }

  // Normalize encoding (stable ordering).
  values.sort_by_key(pair => {
    let (k, _v) = pair
    k
  })
  deleted_keys.sort()

  let w = BytesWriter::new()
  w.write_bytes(postcard_encode_map_string_common_value(values))
  w.write_bytes(postcard_encode_vec_string(deleted_keys))
  w.write_bytes(encode_peer_table(peers))
  for i in 0..<keys.length() {
    w.write_uleb128_u64(peer_idxs[i])
    w.write_uleb128_u64(lamports[i])
  }
  w.to_bytes()
}

pub fn transcode_list_state_snapshot(bytes: BytesView, validate: Bool) -> Bytes raise DecodeError {
  let (values, rest1) = postcard_take_vec_common_value(bytes)
  let (peers, rest2) = take_peer_table(rest1)
  let cols = decode_columnar_vec_maybe_wrapped(rest2)
  if cols.length() != 3 {
    raise DecodeError("list_state: invalid id column count")
  }

  let peer_idx = decode_delta_rle_usize(cols[0])
  let counter = decode_delta_rle_i32(cols[1])
  let lamport_sub = decode_delta_rle_i32(cols[2])

  if validate {
    if peer_idx.length() != values.length() {
      raise DecodeError("list_state: id length mismatch")
    }
    if counter.length() != values.length() || lamport_sub.length() != values.length() {
      raise DecodeError("list_state: id length mismatch")
    }
    for p in peer_idx {
      if p > 0x7FFF_FFFFUL {
        raise DecodeError("list_state: peer_idx too large")
      }
      if p.to_int() < 0 || p.to_int() >= peers.length() {
        raise DecodeError("list_state: peer_idx out of range")
      }
    }
  }

  let ids_bytes = encode_columnar_vec_wrapped([
    encode_delta_rle_usize(peer_idx),
    encode_delta_rle_i32(counter),
    encode_delta_rle_i32(lamport_sub),
  ])

  let w = BytesWriter::new()
  w.write_bytes(postcard_encode_vec_common_value(values))
  w.write_bytes(encode_peer_table(peers))
  w.write_bytes(ids_bytes)
  w.to_bytes()
}

// --- RichtextState (Text) ---

type EncodedMark = (UInt64, CommonValue, Byte) // (key_idx, value, info)

fn take_postcard_vec_marks(bytes: BytesView) -> (Array[EncodedMark], BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let n_u64 = r.read_varint_u64()
  if n_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("richtext: too many marks")
  }
  let n = n_u64.to_int()
  let out: Array[EncodedMark] = []
  for _i in 0..<n {
    let key_idx = r.read_varint_u64()
    let (value, rest) = postcard_take_common_value(r.remaining_view())
    r.skip(r.remaining() - rest.length())
    let info = r.read_u8()
    out.push((key_idx, value, info))
  }
  (out, r.remaining_view())
}

fn encode_postcard_vec_marks(marks: Array[EncodedMark]) -> Bytes {
  let w = BytesWriter::new()
  w.write_varint_u64(marks.length().to_uint64())
  for m in marks {
    let (key_idx, value, info) = m
    w.write_varint_u64(key_idx)
    w.write_bytes(postcard_encode_common_value(value))
    w.write_u8(info)
  }
  w.to_bytes()
}

fn encode_encoded_text(
  span_peer_idx: Array[UInt64],
  span_counter: Array[Int],
  span_lamport_sub: Array[Int],
  span_len: Array[Int],
  keys: Array[String],
  marks: Array[EncodedMark],
) -> Bytes {
  let span_cols = encode_columnar_vec([
    encode_delta_rle_usize(span_peer_idx),
    encode_delta_rle_i32(span_counter),
    encode_delta_rle_i32(span_lamport_sub),
    encode_delta_rle_i32(span_len),
  ])

  let w = BytesWriter::new()
  w.write_varint_u64(3UL)
  w.write_bytes(span_cols)
  w.write_bytes(postcard_encode_vec_string(keys))
  w.write_bytes(encode_postcard_vec_marks(marks))
  w.to_bytes()
}

pub fn transcode_richtext_state_snapshot(bytes: BytesView, validate: Bool) -> Bytes raise DecodeError {
  let (text, rest1) = postcard_take_string(bytes)
  let (peers, rest2) = take_peer_table(rest1)
  let r = BytesReader::from_view(rest2)

  let n_fields = r.read_varint_u64()
  if n_fields != 3UL {
    raise DecodeError("richtext: invalid EncodedText field count")
  }

  let spans_view = r.remaining_view()
  let (span_cols, rest_after_span) = take_columnar_vec(spans_view)
  r.skip(spans_view.length() - rest_after_span.length())
  if span_cols.length() != 4 {
    raise DecodeError("richtext: invalid span column count")
  }
  let span_peer_idx = decode_delta_rle_usize(span_cols[0])
  let span_counter = decode_delta_rle_i32(span_cols[1])
  let span_lamport_sub = decode_delta_rle_i32(span_cols[2])
  let span_len = decode_delta_rle_i32(span_cols[3])
  let span_n = span_len.length()
  if span_peer_idx.length() != span_n || span_counter.length() != span_n || span_lamport_sub.length() != span_n {
    raise DecodeError("richtext: span column length mismatch")
  }

  let keys_view = r.remaining_view()
  let (keys, rest_after_keys) = postcard_take_vec_string(keys_view)
  r.skip(keys_view.length() - rest_after_keys.length())

  let marks_view = r.remaining_view()
  let (marks, rest_after_marks) = take_postcard_vec_marks(marks_view)
  r.skip(marks_view.length() - rest_after_marks.length())

  if r.remaining() != 0 {
    raise DecodeError("richtext: trailing bytes")
  }

  if validate {
    // marks count must match number of span.len == 0
    let mut mark_needed = 0
    let mut text_len_sum = 0
    for i in 0..<span_n {
      let len = span_len[i]
      if len == 0 {
        mark_needed = mark_needed + 1
      } else if len > 0 {
        text_len_sum = text_len_sum + len
      }
      let p = span_peer_idx[i]
      if p > 0x7FFF_FFFFUL || p.to_int() < 0 || p.to_int() >= peers.length() {
        raise DecodeError("richtext: peer_idx out of range")
      }
    }
    if marks.length() != mark_needed {
      raise DecodeError("richtext: mark count mismatch")
    }
    for m in marks {
      let (key_idx, _v, _info) = m
      if key_idx > 0x7FFF_FFFFUL || key_idx.to_int() < 0 || key_idx.to_int() >= keys.length() {
        raise DecodeError("richtext: key_idx out of range")
      }
    }
    if count_utf8_codepoints(text) != text_len_sum {
      raise DecodeError("richtext: text unicode length mismatch")
    }
  }

  let w = BytesWriter::new()
  w.write_bytes(postcard_encode_string(text))
  w.write_bytes(encode_peer_table(peers))
  w.write_bytes(encode_encoded_text(span_peer_idx, span_counter, span_lamport_sub, span_len, keys, marks))
  w.to_bytes()
}

// --- TreeState ---

pub fn transcode_tree_state_snapshot(bytes: BytesView, validate: Bool) -> Bytes raise DecodeError {
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
  let (fractional_indexes_bytes, rest_after_frac) = postcard_take_bytes(frac_view)
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
      if parent > (n_nodes.to_uint64() + 1UL) {
        raise DecodeError("tree: invalid parent_idx_plus_two")
      }
      let fi = fractional_idx_idx[i]
      if fi > 0x7FFF_FFFFUL || fi.to_int() < 0 || fi.to_int() >= positions.length() {
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
  encoded_tree.write_bytes(postcard_encode_bytes(encode_position_arena(positions)))
  encoded_tree.write_bytes(postcard_encode_bytes(reserved_bytes))

  let w = BytesWriter::new()
  w.write_bytes(encode_peer_table(peers))
  w.write_bytes(encoded_tree.to_bytes())
  w.to_bytes()
}

// --- MovableListState ---

pub fn transcode_movable_list_state_snapshot(bytes: BytesView, validate: Bool) -> Bytes raise DecodeError {
  let (values, rest1) = postcard_take_vec_common_value(bytes)
  let (peers, rest2) = take_peer_table(rest1)
  let r = BytesReader::from_view(rest2)
  let n_fields = r.read_varint_u64()
  if n_fields != 4UL {
    raise DecodeError("movable_list: invalid EncodedFastSnapshot field count")
  }

  let items_view = r.remaining_view()
  let (items_cols, rest_after_items) = take_columnar_vec(items_view)
  r.skip(items_view.length() - rest_after_items.length())
  if items_cols.length() != 3 {
    raise DecodeError("movable_list: invalid items column count")
  }
  let invisible_list_item = decode_delta_rle_usize(items_cols[0])
  let pos_id_eq_elem_id = decode_bool_rle(items_cols[1])
  let elem_id_eq_last_set_id = decode_bool_rle(items_cols[2])
  let n_items = invisible_list_item.length()
  if pos_id_eq_elem_id.length() != n_items || elem_id_eq_last_set_id.length() != n_items {
    raise DecodeError("movable_list: items column length mismatch")
  }

  let list_ids_view = r.remaining_view()
  let (list_id_cols, rest_after_list_ids) = take_columnar_vec(list_ids_view)
  r.skip(list_ids_view.length() - rest_after_list_ids.length())
  if list_id_cols.length() != 3 {
    raise DecodeError("movable_list: invalid list_item_ids column count")
  }
  let list_peer_idx = decode_delta_rle_usize(list_id_cols[0])
  let list_counter = decode_delta_rle_i32(list_id_cols[1])
  let list_lamport_sub = decode_delta_rle_i32(list_id_cols[2])
  if list_peer_idx.length() != list_counter.length() || list_peer_idx.length() != list_lamport_sub.length() {
    raise DecodeError("movable_list: list_item_ids length mismatch")
  }

  let elem_ids_view = r.remaining_view()
  let (elem_id_cols, rest_after_elem_ids) = take_columnar_vec(elem_ids_view)
  r.skip(elem_ids_view.length() - rest_after_elem_ids.length())
  if elem_id_cols.length() != 2 {
    raise DecodeError("movable_list: invalid elem_ids column count")
  }
  let elem_peer_idx = decode_delta_rle_usize(elem_id_cols[0])
  let elem_lamport = decode_delta_rle_u32(elem_id_cols[1])
  if elem_peer_idx.length() != elem_lamport.length() {
    raise DecodeError("movable_list: elem_ids length mismatch")
  }

  let last_ids_view = r.remaining_view()
  let (last_id_cols, rest_after_last_ids) = take_columnar_vec(last_ids_view)
  r.skip(last_ids_view.length() - rest_after_last_ids.length())
  if last_id_cols.length() != 2 {
    raise DecodeError("movable_list: invalid last_set_ids column count")
  }
  let last_peer_idx = decode_delta_rle_usize(last_id_cols[0])
  let last_lamport = decode_delta_rle_u32(last_id_cols[1])
  if last_peer_idx.length() != last_lamport.length() {
    raise DecodeError("movable_list: last_set_ids length mismatch")
  }

  if r.remaining() != 0 {
    raise DecodeError("movable_list: trailing bytes")
  }

  if validate {
    // visible values should match visible entries = items.len - 1 (sentinel excluded)
    let visible_n = if n_items == 0 { 0 } else { n_items - 1 }
    if values.length() != visible_n {
      raise DecodeError("movable_list: visible value count mismatch")
    }

    // list_item_ids count matches (visible + invisible) list items
    let mut expected_list_ids = 0UL
    if n_items > 0 {
      expected_list_ids = (visible_n).to_uint64()
      for c in invisible_list_item {
        expected_list_ids = expected_list_ids + c
      }
    }
    if list_peer_idx.length().to_uint64() != expected_list_ids {
      raise DecodeError("movable_list: list_item_ids count mismatch")
    }

    let mut need_elem_ids = 0
    let mut need_last_ids = 0
    for i in 1..<n_items {
      if !pos_id_eq_elem_id[i] {
        need_elem_ids = need_elem_ids + 1
      }
      if !elem_id_eq_last_set_id[i] {
        need_last_ids = need_last_ids + 1
      }
    }
    if elem_peer_idx.length() != need_elem_ids || last_peer_idx.length() != need_last_ids {
      raise DecodeError("movable_list: optional id count mismatch")
    }

    for p in list_peer_idx {
      if p > 0x7FFF_FFFFUL || p.to_int() < 0 || p.to_int() >= peers.length() {
        raise DecodeError("movable_list: peer_idx out of range")
      }
    }
    for p in elem_peer_idx {
      if p > 0x7FFF_FFFFUL || p.to_int() < 0 || p.to_int() >= peers.length() {
        raise DecodeError("movable_list: elem peer_idx out of range")
      }
    }
    for p in last_peer_idx {
      if p > 0x7FFF_FFFFUL || p.to_int() < 0 || p.to_int() >= peers.length() {
        raise DecodeError("movable_list: last peer_idx out of range")
      }
    }
  }

  let items_bytes = encode_columnar_vec([
    encode_delta_rle_usize(invisible_list_item),
    encode_bool_rle(pos_id_eq_elem_id),
    encode_bool_rle(elem_id_eq_last_set_id),
  ])
  let list_ids_bytes = encode_columnar_vec([
    encode_delta_rle_usize(list_peer_idx),
    encode_delta_rle_i32(list_counter),
    encode_delta_rle_i32(list_lamport_sub),
  ])
  let elem_ids_bytes = encode_columnar_vec([
    encode_delta_rle_usize(elem_peer_idx),
    encode_delta_rle_u32(elem_lamport),
  ])
  let last_ids_bytes = encode_columnar_vec([
    encode_delta_rle_usize(last_peer_idx),
    encode_delta_rle_u32(last_lamport),
  ])

  let encoded = BytesWriter::new()
  encoded.write_varint_u64(4UL)
  encoded.write_bytes(items_bytes)
  encoded.write_bytes(list_ids_bytes)
  encoded.write_bytes(elem_ids_bytes)
  encoded.write_bytes(last_ids_bytes)

  let w = BytesWriter::new()
  w.write_bytes(postcard_encode_vec_common_value(values))
  w.write_bytes(encode_peer_table(peers))
  w.write_bytes(encoded.to_bytes())
  w.to_bytes()
}

// --- CounterState ---

pub fn transcode_counter_state_snapshot(bytes: BytesView, _validate: Bool) -> Bytes raise DecodeError {
  if bytes.length() != 8 {
    raise DecodeError("counter_state: invalid length")
  }
  // f64 little-endian, keep exact bits.
  let bits = BytesReader::from_view(bytes).read_u64_le()
  let w = BytesWriter::new()
  w.write_u64_le(bits)
  w.to_bytes()
}

pub fn transcode_container_state_snapshot(
  kind: ContainerType,
  payload: BytesView,
  validate: Bool,
) -> Bytes raise DecodeError {
  match kind {
    ContainerType::Map => transcode_map_state_snapshot(payload, validate)
    ContainerType::List => transcode_list_state_snapshot(payload, validate)
    ContainerType::Text => transcode_richtext_state_snapshot(payload, validate)
    ContainerType::Tree => transcode_tree_state_snapshot(payload, validate)
    ContainerType::MovableList => transcode_movable_list_state_snapshot(payload, validate)
    ContainerType::Counter => transcode_counter_state_snapshot(payload, validate)
    ContainerType::Unknown(_) => payload.to_bytes()
  }
}
