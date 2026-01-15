pub fn decode_delta_rle_u32(bytes: BytesView) -> Array[UInt] raise DecodeError {
  let deltas = decode_any_rle(bytes, read_postcard_i128_bigint)
  let out: Array[UInt] = []
  let mut abs: BigInt = 0N
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

pub fn encode_delta_rle_u32(values: Array[UInt]) -> Bytes {
  if values.length() == 0 {
    return b""
  }
  let deltas: Array[BigInt] = []
  let mut prev: BigInt = 0N
  for v in values {
    let cur = BigInt::from_uint(v)
    deltas.push(cur - prev)
    prev = cur
  }
  encode_any_rle_literal(deltas, write_postcard_i128_bigint)
}

pub fn decode_delta_rle_i32(bytes: BytesView) -> Array[Int] raise DecodeError {
  let deltas = decode_any_rle(bytes, read_postcard_i128_bigint)
  let out: Array[Int] = []
  let mut abs: BigInt = 0N
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

pub fn encode_delta_rle_i32(values: Array[Int]) -> Bytes {
  if values.length() == 0 {
    return b""
  }
  let deltas: Array[BigInt] = []
  let mut prev: BigInt = 0N
  for v in values {
    let cur = BigInt::from_int(v)
    deltas.push(cur - prev)
    prev = cur
  }
  encode_any_rle_literal(deltas, write_postcard_i128_bigint)
}

pub fn decode_delta_rle_usize(bytes: BytesView) -> Array[UInt64] raise DecodeError {
  let deltas = decode_any_rle(bytes, read_postcard_i128_bigint)
  let out: Array[UInt64] = []
  let mut abs: BigInt = 0N
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

pub fn encode_delta_rle_usize(values: Array[UInt64]) -> Bytes {
  if values.length() == 0 {
    return b""
  }
  let deltas: Array[BigInt] = []
  let mut prev: BigInt = 0N
  for v in values {
    let cur = BigInt::from_uint64(v)
    deltas.push(cur - prev)
    prev = cur
  }
  encode_any_rle_literal(deltas, write_postcard_i128_bigint)
}

pub fn decode_delta_rle_isize(bytes: BytesView) -> Array[Int64] raise DecodeError {
  let deltas = decode_any_rle(bytes, read_postcard_i128_bigint)
  let out: Array[Int64] = []
  let mut abs: BigInt = 0N
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

pub fn encode_delta_rle_isize(values: Array[Int64]) -> Bytes {
  if values.length() == 0 {
    return b""
  }
  let deltas: Array[BigInt] = []
  let mut prev: BigInt = 0N
  for v in values {
    let cur = BigInt::from_int64(v)
    deltas.push(cur - prev)
    prev = cur
  }
  encode_any_rle_literal(deltas, write_postcard_i128_bigint)
}

fn read_postcard_option_i64(r: BytesReader) -> Int64? raise DecodeError {
  let tag = r.read_varint_u64()
  match tag {
    0UL => None
    1UL => Some(r.read_varint_i64())
    _ => raise DecodeError("postcard: invalid Option tag")
  }
}

fn write_postcard_option_i64(w: BytesWriter, v: Int64?) -> Unit {
  match v {
    None => w.write_varint_u64(0)
    Some(x) => {
      w.write_varint_u64(1)
      w.write_varint_i64(x)
    }
  }
}

priv struct BitReader {
  buf: BytesView
  mut bit_pos: Int
  total_bits: Int
}

fn BitReader::new(buf: BytesView, last_used_bits: UInt) -> BitReader raise DecodeError {
  if last_used_bits > 8 {
    raise DecodeError("delta_of_delta: invalid last_used_bits")
  }
  if last_used_bits == 0 {
    if buf.length() != 0 {
      raise DecodeError("delta_of_delta: unexpected bitstream bytes")
    }
    return { buf, bit_pos: 0, total_bits: 0 }
  }
  if buf.length() == 0 {
    raise DecodeError("delta_of_delta: missing bitstream bytes")
  }
  let last_bits = last_used_bits.reinterpret_as_int()
  let total_bits = (buf.length() - 1) * 8 + last_bits
  { buf, bit_pos: 0, total_bits }
}

fn BitReader::remaining(self: BitReader) -> Int {
  self.total_bits - self.bit_pos
}

fn BitReader::read_bit(self: BitReader) -> Bool raise DecodeError {
  if self.bit_pos >= self.total_bits {
    raise DecodeError("delta_of_delta: unexpected end of bitstream")
  }
  let byte_index = self.bit_pos / 8
  let bit_in_byte = self.bit_pos % 8
  self.bit_pos = self.bit_pos + 1
  let mask = (1 << (7 - bit_in_byte)).to_byte()
  (self.buf[byte_index] & mask) != b'\x00'
}

fn BitReader::read_bits_u64(self: BitReader, n: Int) -> UInt64 raise DecodeError {
  if n < 0 || n > 64 {
    raise DecodeError("delta_of_delta: invalid bit width")
  }
  let mut v: UInt64 = 0
  for _i in 0..<n {
    v = (v << 1) | (if self.read_bit() { 1UL } else { 0UL })
  }
  v
}

fn decode_delta_of_delta_value(br: BitReader) -> Int64 raise DecodeError {
  let b0 = br.read_bit()
  if !b0 {
    return 0
  }
  let b1 = br.read_bit()
  if !b1 {
    let v = br.read_bits_u64(7).reinterpret_as_int64()
    return v - 63L
  }
  let b2 = br.read_bit()
  if !b2 {
    let v = br.read_bits_u64(9).reinterpret_as_int64()
    return v - 255L
  }
  let b3 = br.read_bit()
  if !b3 {
    let v = br.read_bits_u64(12).reinterpret_as_int64()
    return v - 2047L
  }
  let b4 = br.read_bit()
  if !b4 {
    let v = br.read_bits_u64(21).reinterpret_as_int64()
    return v - 1048575L
  }
  br.read_bits_u64(64).reinterpret_as_int64()
}
