fn longest_common_prefix_length(a: Bytes, b: Bytes) -> UInt64 {
  let n = if a.length() < b.length() { a.length() } else { b.length() }
  let mut i = 0
  while i < n {
    if a[i] != b[i] {
      break
    }
    i = i + 1
  }
  i.to_uint64()
}

pub fn decode_position_arena_v2(bytes: BytesView) -> Array[Bytes] raise DecodeError {
  if bytes.length() == 0 {
    return []
  }
  let cols = decode_columnar_vec(bytes)
  if cols.length() != 2 {
    raise DecodeError("position_arena: invalid column count")
  }

  let common_prefix_u64 = decode_any_rle_usize(cols[0])
  let n = common_prefix_u64.length()
  let rest_col = cols[1]

  let r = BytesReader::from_view(rest_col)
  let rests: Array[Bytes] = []
  for _i in 0..<n {
    let len_u64 = r.read_varint_u64()
    if len_u64 > 0x7FFF_FFFFUL {
      raise DecodeError("position_arena: rest too large")
    }
    let len = len_u64.to_int()
    if len < 0 || len > r.remaining() {
      raise DecodeError("position_arena: invalid rest length")
    }
    rests.push(r.read_exact(len).to_bytes())
  }
  if r.remaining() != 0 {
    raise DecodeError("position_arena: trailing bytes")
  }
  if rests.length() != n {
    raise DecodeError("position_arena: invalid rest count")
  }

  let out: Array[Bytes] = []
  let mut last: Bytes = b""
  for i in 0..<n {
    let common_u64 = common_prefix_u64[i]
    if common_u64 > 0x7FFF_FFFFUL {
      raise DecodeError("position_arena: common_prefix too large")
    }
    let common = common_u64.to_int()
    if common < 0 || common > last.length() {
      raise DecodeError("position_arena: invalid common_prefix")
    }
    let w = BytesWriter::new()
    w.write_bytesview(last[0:common])
    w.write_bytes(rests[i])
    last = w.to_bytes()
    out.push(last)
  }
  out
}

pub fn encode_position_arena_v2(positions: Array[Bytes]) -> Bytes {
  if positions.length() == 0 {
    return b""
  }
  let common_prefix: Array[UInt64] = []
  let rest_col_w = BytesWriter::new()
  let mut last: Bytes = b""
  for pos in positions {
    let common = longest_common_prefix_length(last, pos)
    common_prefix.push(common)
    let rest = pos[common.to_int():pos.length()]
    rest_col_w.write_varint_u64(rest.length().to_uint64())
    rest_col_w.write_bytesview(rest)
    last = pos
  }

  let col0 = encode_any_rle_usize(common_prefix)
  let col1 = rest_col_w.to_bytes()
  encode_columnar_vec([col0, col1])
}

