///|
pub fn decode_bool_rle(bytes : BytesView) -> Array[Bool] raise DecodeError {
  if bytes.length() == 0 {
    return []
  }
  let r = BytesReader::from_view(bytes)
  let out : Array[Bool] = []
  let mut state = false
  while r.remaining() > 0 {
    let len_u64 = r.read_varint_u64()
    if len_u64 > 0x7FFF_FFFFUL {
      raise DecodeError("boolrle: run too long")
    }
    let len = len_u64.to_int()
    if len < 0 {
      raise DecodeError("boolrle: invalid run length")
    }
    for _i in 0..<len {
      out.push(state)
    }
    state = !state
  }
  out
}

///|
pub fn encode_bool_rle(values : Array[Bool]) -> Bytes {
  let w = BytesWriter::new()
  if values.length() == 0 {
    return w.to_bytes()
  }
  let mut state = false
  let mut run_len : UInt64 = 0
  for v in values {
    if v == state {
      run_len = run_len + 1
    } else {
      w.write_varint_u64(run_len)
      state = !state
      run_len = 1
    }
  }
  w.write_varint_u64(run_len)
  w.to_bytes()
}

///|
pub fn bool_rle_take_n_finalize(
  bytes : BytesView,
  n : Int,
) -> (Array[Bool], BytesView) raise DecodeError {
  if n < 0 {
    raise DecodeError("boolrle: invalid n")
  }
  if n == 0 {
    return ([], bytes)
  }
  let r = BytesReader::from_view(bytes)
  let out : Array[Bool] = []
  let mut state = false
  while out.length() < n {
    if r.remaining() == 0 {
      raise DecodeError("boolrle: not enough elements")
    }
    let len_u64 = r.read_varint_u64()
    if len_u64 > 0x7FFF_FFFFUL {
      raise DecodeError("boolrle: run too long")
    }
    let len = len_u64.to_int()
    if len < 0 {
      raise DecodeError("boolrle: invalid run length")
    }
    if out.length() + len > n {
      raise DecodeError("boolrle: too many elements")
    }
    for _i in 0..<len {
      out.push(state)
    }
    state = !state
  }
  (out, r.remaining_view())
}
