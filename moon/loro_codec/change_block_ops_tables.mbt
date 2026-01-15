///|
pub struct EncodedOpRow {
  container_index : UInt
  prop : Int
  value_type : UInt
  len : UInt
} derive(Eq, Show)

///|
pub fn decode_encoded_ops(
  bytes : BytesView,
) -> Array[EncodedOpRow] raise DecodeError {
  if bytes.length() == 0 {
    return []
  }
  let cols = decode_columnar_vec_maybe_wrapped(bytes)
  if cols.length() != 4 {
    raise DecodeError("encoded_ops: invalid column count")
  }
  let container_indices = decode_delta_rle_u32(cols[0])
  let props = decode_delta_rle_i32(cols[1])
  let value_types = decode_rle_u8(cols[2])
  let lens = decode_rle_u32(cols[3])
  let n = container_indices.length()
  if props.length() != n || value_types.length() != n || lens.length() != n {
    raise DecodeError("encoded_ops: column length mismatch")
  }
  let out : Array[EncodedOpRow] = []
  for i in 0..<n {
    out.push({
      container_index: container_indices[i],
      prop: props[i],
      value_type: value_types[i],
      len: lens[i],
    })
  }
  out
}

///|
pub fn encode_encoded_ops(ops : Array[EncodedOpRow]) -> Bytes {
  if ops.length() == 0 {
    return encode_columnar_vec_wrapped([b"", b"", b"", b""])
  }
  let container_indices : Array[UInt] = []
  let props : Array[Int] = []
  let value_types : Array[UInt] = []
  let lens : Array[UInt] = []
  for op in ops {
    container_indices.push(op.container_index)
    props.push(op.prop)
    value_types.push(op.value_type)
    lens.push(op.len)
  }
  let col0 = encode_delta_rle_u32(container_indices)
  let col1 = encode_delta_rle_i32(props)
  let col2 = encode_rle_u8(value_types)
  let col3 = encode_rle_u32(lens)
  encode_columnar_vec_wrapped([col0, col1, col2, col3])
}

///|
pub struct EncodedDeleteStartIdRow {
  peer_idx : UInt64
  counter : Int
  len : Int64
} derive(Eq, Show)

///|
pub fn decode_delete_start_ids(
  bytes : BytesView,
) -> Array[EncodedDeleteStartIdRow] raise DecodeError {
  if bytes.length() == 0 {
    return []
  }
  let cols = decode_columnar_vec_maybe_wrapped(bytes)
  if cols.length() != 3 {
    raise DecodeError("delete_start_ids: invalid column count")
  }
  let peer_idxs = decode_delta_rle_usize(cols[0])
  let counters = decode_delta_rle_i32(cols[1])
  let lens = decode_delta_rle_isize(cols[2])
  let n = peer_idxs.length()
  if counters.length() != n || lens.length() != n {
    raise DecodeError("delete_start_ids: column length mismatch")
  }
  let out : Array[EncodedDeleteStartIdRow] = []
  for i in 0..<n {
    out.push({ peer_idx: peer_idxs[i], counter: counters[i], len: lens[i] })
  }
  out
}

///|
pub fn encode_delete_start_ids(ids : Array[EncodedDeleteStartIdRow]) -> Bytes {
  if ids.length() == 0 {
    return b""
  }
  let peer_idxs : Array[UInt64] = []
  let counters : Array[Int] = []
  let lens : Array[Int64] = []
  for id in ids {
    peer_idxs.push(id.peer_idx)
    counters.push(id.counter)
    lens.push(id.len)
  }
  let col0 = encode_delta_rle_usize(peer_idxs)
  let col1 = encode_delta_rle_i32(counters)
  let col2 = encode_delta_rle_isize(lens)
  encode_columnar_vec_wrapped([col0, col1, col2])
}
