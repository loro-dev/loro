///|
pub fn decode_delta_rle_u32(bytes : BytesView) -> Array[UInt] raise DecodeError {
  let deltas = decode_any_rle(bytes, read_postcard_i128_bigint)
  let out : Array[UInt] = []
  let mut abs : BigInt = 0N
  let max = BigInt::from_uint64(0xFFFF_FFFFUL)
  for d in deltas {
    abs = abs + d
    if abs < 0N || abs > max {
      raise DecodeError("deltarle: u32 overflow")
    }
    out.push(abs.to_uint())
  }
  out
}

///|
pub fn encode_delta_rle_u32(values : Array[UInt]) -> Bytes {
  if values.length() == 0 {
    return b""
  }
  let deltas : Array[BigInt] = []
  let mut prev : BigInt = 0N
  for v in values {
    let cur = BigInt::from_uint(v)
    deltas.push(cur - prev)
    prev = cur
  }
  encode_any_rle_literal(deltas, write_postcard_i128_bigint)
}

///|
pub fn decode_delta_rle_i32(bytes : BytesView) -> Array[Int] raise DecodeError {
  let deltas = decode_any_rle(bytes, read_postcard_i128_bigint)
  let out : Array[Int] = []
  let mut abs : BigInt = 0N
  let min = BigInt::from_int64(-2147483648L)
  let max = BigInt::from_int64(2147483647L)
  for d in deltas {
    abs = abs + d
    if abs < min || abs > max {
      raise DecodeError("deltarle: i32 overflow")
    }
    out.push(abs.to_int())
  }
  out
}

///|
pub fn encode_delta_rle_i32(values : Array[Int]) -> Bytes {
  if values.length() == 0 {
    return b""
  }
  let deltas : Array[BigInt] = []
  let mut prev : BigInt = 0N
  for v in values {
    let cur = BigInt::from_int(v)
    deltas.push(cur - prev)
    prev = cur
  }
  encode_any_rle_literal(deltas, write_postcard_i128_bigint)
}

///|
pub fn decode_delta_rle_usize(
  bytes : BytesView,
) -> Array[UInt64] raise DecodeError {
  let deltas = decode_any_rle(bytes, read_postcard_i128_bigint)
  let out : Array[UInt64] = []
  let mut abs : BigInt = 0N
  let max = BigInt::from_uint64(0xFFFF_FFFF_FFFF_FFFFUL)
  for d in deltas {
    abs = abs + d
    if abs < 0N || abs > max {
      raise DecodeError("deltarle: usize overflow")
    }
    out.push(abs.to_uint64())
  }
  out
}

///|
pub fn encode_delta_rle_usize(values : Array[UInt64]) -> Bytes {
  if values.length() == 0 {
    return b""
  }
  let deltas : Array[BigInt] = []
  let mut prev : BigInt = 0N
  for v in values {
    let cur = BigInt::from_uint64(v)
    deltas.push(cur - prev)
    prev = cur
  }
  encode_any_rle_literal(deltas, write_postcard_i128_bigint)
}

///|
pub fn decode_delta_rle_isize(
  bytes : BytesView,
) -> Array[Int64] raise DecodeError {
  let deltas = decode_any_rle(bytes, read_postcard_i128_bigint)
  let out : Array[Int64] = []
  let mut abs : BigInt = 0N
  let min = BigInt::from_int64(-9223372036854775808L)
  let max = BigInt::from_int64(9223372036854775807L)
  for d in deltas {
    abs = abs + d
    if abs < min || abs > max {
      raise DecodeError("deltarle: isize overflow")
    }
    out.push(abs.to_int64())
  }
  out
}

///|
pub fn encode_delta_rle_isize(values : Array[Int64]) -> Bytes {
  if values.length() == 0 {
    return b""
  }
  let deltas : Array[BigInt] = []
  let mut prev : BigInt = 0N
  for v in values {
    let cur = BigInt::from_int64(v)
    deltas.push(cur - prev)
    prev = cur
  }
  encode_any_rle_literal(deltas, write_postcard_i128_bigint)
}
