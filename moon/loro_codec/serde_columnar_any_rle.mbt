///|
pub fn decode_rle_u8(bytes : BytesView) -> Array[UInt] raise DecodeError {
  decode_any_rle(bytes, read_postcard_u8)
}

///|
pub fn encode_rle_u8(values : Array[UInt]) -> Bytes {
  encode_any_rle_literal(values, write_postcard_u8)
}

///|
pub fn decode_rle_u32(bytes : BytesView) -> Array[UInt] raise DecodeError {
  decode_any_rle(bytes, read_postcard_u32)
}

///|
pub fn encode_rle_u32(values : Array[UInt]) -> Bytes {
  encode_any_rle_literal(values, write_postcard_u32)
}

///|
pub fn decode_any_rle_usize(
  bytes : BytesView,
) -> Array[UInt64] raise DecodeError {
  decode_any_rle(bytes, read_postcard_usize)
}

///|
pub fn any_rle_take_n_finalize_usize(
  bytes : BytesView,
  n : Int,
) -> (Array[UInt64], BytesView) raise DecodeError {
  any_rle_take_n_finalize(bytes, n, read_postcard_usize)
}

///|
pub fn encode_any_rle_usize(values : Array[UInt64]) -> Bytes {
  encode_any_rle_literal(values, write_postcard_usize)
}

///|
pub fn decode_any_rle_u32(bytes : BytesView) -> Array[UInt] raise DecodeError {
  decode_any_rle(bytes, read_postcard_u32)
}

///|
pub fn any_rle_take_n_finalize_u32(
  bytes : BytesView,
  n : Int,
) -> (Array[UInt], BytesView) raise DecodeError {
  any_rle_take_n_finalize(bytes, n, read_postcard_u32)
}

///|
pub fn encode_any_rle_u32(values : Array[UInt]) -> Bytes {
  encode_any_rle_literal(values, write_postcard_u32)
}

///|
pub fn decode_any_rle_u64(bytes : BytesView) -> Array[UInt64] raise DecodeError {
  decode_any_rle(bytes, read_postcard_u64)
}

///|
pub fn encode_any_rle_u64(values : Array[UInt64]) -> Bytes {
  encode_any_rle_literal(values, write_postcard_u64)
}

///|
pub fn decode_any_rle_u8(bytes : BytesView) -> Array[UInt] raise DecodeError {
  decode_any_rle(bytes, read_postcard_u8)
}

///|
pub fn any_rle_take_n_finalize_u8(
  bytes : BytesView,
  n : Int,
) -> (Array[UInt], BytesView) raise DecodeError {
  any_rle_take_n_finalize(bytes, n, read_postcard_u8)
}

///|
pub fn encode_any_rle_u8(values : Array[UInt]) -> Bytes {
  encode_any_rle_literal(values, write_postcard_u8)
}

///|
pub fn decode_any_rle_i32(bytes : BytesView) -> Array[Int] raise DecodeError {
  decode_any_rle(bytes, read_postcard_i32)
}

///|
pub fn any_rle_take_n_finalize_i32(
  bytes : BytesView,
  n : Int,
) -> (Array[Int], BytesView) raise DecodeError {
  any_rle_take_n_finalize(bytes, n, read_postcard_i32)
}

///|
pub fn encode_any_rle_i32(values : Array[Int]) -> Bytes {
  encode_any_rle_literal(values, write_postcard_i32)
}

///|
fn read_varint_u128_bigint(r : BytesReader) -> BigInt raise DecodeError {
  let mut result : BigInt = 0N
  let mut shift = 0
  for _i in 0..<19 {
    let byte = r.read_u8().to_uint()
    let chunk = BigInt::from_uint(byte & 0x7F)
    result = result | (chunk << shift)
    if (byte & 0x80) == 0 {
      return result
    }
    shift = shift + 7
  }
  raise DecodeError("varint too long")
}

///|
fn write_varint_u128_bigint(w : BytesWriter, value : BigInt) -> Unit {
  let mut v = value
  while true {
    let byte = (v & 0x7FN).to_uint()
    v = v >> 7
    if v != 0N {
      w.write_u8((byte | 0x80).to_byte())
    } else {
      w.write_u8(byte.to_byte())
      break
    }
  }
}

///|
fn zigzag_decode_bigint(encoded : BigInt) -> BigInt {
  if (encoded & 1N) == 0N {
    encoded >> 1
  } else {
    -((encoded >> 1) + 1N)
  }
}

///|
fn zigzag_encode_bigint(value : BigInt) -> BigInt {
  if value >= 0N {
    value << 1
  } else {
    (-value << 1) - 1N
  }
}

///|
fn read_postcard_i128_bigint(r : BytesReader) -> BigInt raise DecodeError {
  zigzag_decode_bigint(read_varint_u128_bigint(r))
}

///|
fn write_postcard_i128_bigint(w : BytesWriter, v : BigInt) -> Unit {
  write_varint_u128_bigint(w, zigzag_encode_bigint(v))
}
