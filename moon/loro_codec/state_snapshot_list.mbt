///|
pub fn transcode_list_state_snapshot(
  bytes : BytesView,
  validate : Bool,
) -> Bytes raise DecodeError {
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
    if counter.length() != values.length() ||
      lamport_sub.length() != values.length() {
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
