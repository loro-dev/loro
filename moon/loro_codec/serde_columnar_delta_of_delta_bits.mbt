///|
priv struct BitReader {
  buf : BytesView
  mut bit_pos : Int
  total_bits : Int
}

///|
fn BitReader::new(
  buf : BytesView,
  last_used_bits : UInt,
) -> BitReader raise DecodeError {
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

///|
fn BitReader::remaining(self : BitReader) -> Int {
  self.total_bits - self.bit_pos
}

///|
fn BitReader::read_bit(self : BitReader) -> Bool raise DecodeError {
  if self.bit_pos >= self.total_bits {
    raise DecodeError("delta_of_delta: unexpected end of bitstream")
  }
  let byte_index = self.bit_pos / 8
  let bit_in_byte = self.bit_pos % 8
  self.bit_pos = self.bit_pos + 1
  let mask = (1 << (7 - bit_in_byte)).to_byte()
  (self.buf[byte_index] & mask) != b'\x00'
}

///|
fn BitReader::read_bits_u64(
  self : BitReader,
  n : Int,
) -> UInt64 raise DecodeError {
  if n < 0 || n > 64 {
    raise DecodeError("delta_of_delta: invalid bit width")
  }
  let mut v : UInt64 = 0
  for _i in 0..<n {
    v = (v << 1) | (if self.read_bit() { 1UL } else { 0UL })
  }
  v
}

///|
fn decode_delta_of_delta_value(br : BitReader) -> Int64 raise DecodeError {
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
