///|
pub fn transcode_movable_list_state_snapshot(
  bytes : BytesView,
  validate : Bool,
) -> Bytes raise DecodeError {
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
  if pos_id_eq_elem_id.length() != n_items ||
    elem_id_eq_last_set_id.length() != n_items {
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
  if list_peer_idx.length() != list_counter.length() ||
    list_peer_idx.length() != list_lamport_sub.length() {
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
      expected_list_ids = visible_n.to_uint64()
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
    if elem_peer_idx.length() != need_elem_ids ||
      last_peer_idx.length() != need_last_ids {
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
