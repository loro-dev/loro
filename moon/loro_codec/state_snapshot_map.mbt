///|
fn collect_sorted_unique_keys(
  values : Array[(String, CommonValue)],
  deleted_keys : Array[String],
  validate : Bool,
) -> Array[String] raise DecodeError {
  let keys : Array[String] = []
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
  let out : Array[String] = []
  out.push(keys[0])
  for i in 1..<keys.length() {
    if keys[i] == keys[i - 1] {
      raise DecodeError("map_state: duplicate key")
    }
    out.push(keys[i])
  }
  out
}

///|
pub fn transcode_map_state_snapshot(
  bytes : BytesView,
  validate : Bool,
) -> Bytes raise DecodeError {
  let (values, rest1) = postcard_take_map_string_common_value(bytes)
  let (deleted_keys, rest2) = postcard_take_vec_string(rest1)
  let (peers, meta_bytes) = take_peer_table(rest2)
  let keys = collect_sorted_unique_keys(values, deleted_keys, validate)
  let meta_r = BytesReader::from_view(meta_bytes)
  let peer_idxs : Array[UInt64] = []
  let lamports : Array[UInt64] = []
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
