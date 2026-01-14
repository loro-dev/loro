pub fn decode_columnar_vec(bytes: BytesView) -> Array[BytesView] raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let n_cols_u64 = r.read_varint_u64()
  if n_cols_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("serde_columnar: too many columns")
  }
  let n_cols = n_cols_u64.to_int()
  let cols: Array[BytesView] = []
  for _i in 0..<n_cols {
    let len_u64 = r.read_varint_u64()
    if len_u64 > 0x7FFF_FFFFUL {
      raise DecodeError("serde_columnar: column too large")
    }
    let len = len_u64.to_int()
    if len < 0 || len > r.remaining() {
      raise DecodeError("serde_columnar: invalid column length")
    }
    cols.push(r.read_exact(len))
  }
  if r.remaining() != 0 {
    raise DecodeError("serde_columnar: trailing bytes")
  }
  cols
}

pub fn encode_columnar_vec(cols: Array[Bytes]) -> Bytes {
  let w = BytesWriter::new()
  w.write_varint_u64(cols.length().to_uint64())
  for col in cols {
    w.write_varint_u64(col.length().to_uint64())
    w.write_bytes(col)
  }
  w.to_bytes()
}

pub fn decode_bool_rle(bytes: BytesView) -> Array[Bool] raise DecodeError {
  if bytes.length() == 0 {
    return []
  }
  let r = BytesReader::from_view(bytes)
  let out: Array[Bool] = []
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

pub fn encode_bool_rle(values: Array[Bool]) -> Bytes {
  let w = BytesWriter::new()
  if values.length() == 0 {
    return w.to_bytes()
  }

  let mut state = false
  let mut run_len: UInt64 = 0
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

pub fn bool_rle_take_n_finalize(bytes: BytesView, n: Int) -> (Array[Bool], BytesView) raise DecodeError {
  if n < 0 {
    raise DecodeError("boolrle: invalid n")
  }
  if n == 0 {
    return ([], bytes)
  }
  let r = BytesReader::from_view(bytes)
  let out: Array[Bool] = []
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

fn[T] decode_any_rle(
  bytes: BytesView,
  read_value: (BytesReader) -> T raise DecodeError,
) -> Array[T] raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let out: Array[T] = []
  while r.remaining() > 0 {
    let signed_len = r.read_varint_i64()
    if signed_len == 0 {
      raise DecodeError("anyrle: zero length segment")
    }
    if signed_len > 0 {
      let len_u64 = signed_len.reinterpret_as_uint64()
      if len_u64 > 0x7FFF_FFFFUL {
        raise DecodeError("anyrle: run too long")
      }
      let len = len_u64.to_int()
      let v = read_value(r)
      for _i in 0..<len {
        out.push(v)
      }
    } else {
      let len_u64 = (-signed_len).reinterpret_as_uint64()
      if len_u64 > 0x7FFF_FFFFUL {
        raise DecodeError("anyrle: literal too long")
      }
      let len = len_u64.to_int()
      for _i in 0..<len {
        out.push(read_value(r))
      }
    }
  }
  out
}

fn[T] any_rle_take_n_finalize(
  bytes: BytesView,
  n: Int,
  read_value: (BytesReader) -> T raise DecodeError,
) -> (Array[T], BytesView) raise DecodeError {
  if n < 0 {
    raise DecodeError("anyrle: invalid n")
  }
  if n == 0 {
    return ([], bytes)
  }
  let r = BytesReader::from_view(bytes)
  let out: Array[T] = []
  while out.length() < n {
    if r.remaining() == 0 {
      raise DecodeError("anyrle: not enough elements")
    }
    let signed_len = r.read_varint_i64()
    if signed_len == 0 {
      raise DecodeError("anyrle: zero length segment")
    }
    if signed_len > 0 {
      let len_u64 = signed_len.reinterpret_as_uint64()
      if len_u64 > 0x7FFF_FFFFUL {
        raise DecodeError("anyrle: run too long")
      }
      let len = len_u64.to_int()
      if out.length() + len > n {
        raise DecodeError("anyrle: too many elements")
      }
      let v = read_value(r)
      for _i in 0..<len {
        out.push(v)
      }
    } else {
      let len_u64 = (-signed_len).reinterpret_as_uint64()
      if len_u64 > 0x7FFF_FFFFUL {
        raise DecodeError("anyrle: literal too long")
      }
      let len = len_u64.to_int()
      if out.length() + len > n {
        raise DecodeError("anyrle: too many elements")
      }
      for _i in 0..<len {
        out.push(read_value(r))
      }
    }
  }
  (out, r.remaining_view())
}

fn[T] encode_any_rle_literal(
  values: Array[T],
  write_value: (BytesWriter, T) -> Unit,
) -> Bytes {
  let w = BytesWriter::new()
  if values.length() == 0 {
    return w.to_bytes()
  }
  let n = values.length()
  let signed_len = (-n).to_int64()
  w.write_varint_i64(signed_len)
  for v in values {
    write_value(w, v)
  }
  w.to_bytes()
}

fn read_postcard_u8(r: BytesReader) -> UInt raise DecodeError {
  r.read_u8().to_uint()
}

fn write_postcard_u8(w: BytesWriter, v: UInt) -> Unit {
  w.write_u8((v & 0xFF).to_byte())
}

fn read_postcard_u32(r: BytesReader) -> UInt raise DecodeError {
  let v = r.read_varint_u64()
  if v > 0xFFFF_FFFFUL {
    raise DecodeError("postcard: u32 overflow")
  }
  v.to_uint()
}

fn write_postcard_u32(w: BytesWriter, v: UInt) -> Unit {
  w.write_varint_u64(v.to_uint64())
}

fn read_postcard_u64(r: BytesReader) -> UInt64 raise DecodeError {
  r.read_varint_u64()
}

fn write_postcard_u64(w: BytesWriter, v: UInt64) -> Unit {
  w.write_varint_u64(v)
}

fn read_postcard_usize(r: BytesReader) -> UInt64 raise DecodeError {
  r.read_varint_u64()
}

fn write_postcard_usize(w: BytesWriter, v: UInt64) -> Unit {
  w.write_varint_u64(v)
}

fn read_postcard_i32(r: BytesReader) -> Int raise DecodeError {
  let v = r.read_varint_i64()
  if v < -2147483648L || v > 2147483647L {
    raise DecodeError("postcard: i32 overflow")
  }
  v.to_int()
}

fn write_postcard_i32(w: BytesWriter, v: Int) -> Unit {
  w.write_varint_i64(v.to_int64())
}

pub fn decode_rle_u8(bytes: BytesView) -> Array[UInt] raise DecodeError {
  decode_any_rle(bytes, read_postcard_u8)
}

pub fn encode_rle_u8(values: Array[UInt]) -> Bytes {
  encode_any_rle_literal(values, write_postcard_u8)
}

pub fn decode_rle_u32(bytes: BytesView) -> Array[UInt] raise DecodeError {
  decode_any_rle(bytes, read_postcard_u32)
}

pub fn encode_rle_u32(values: Array[UInt]) -> Bytes {
  encode_any_rle_literal(values, write_postcard_u32)
}

pub fn decode_any_rle_usize(bytes: BytesView) -> Array[UInt64] raise DecodeError {
  decode_any_rle(bytes, read_postcard_usize)
}

pub fn any_rle_take_n_finalize_usize(bytes: BytesView, n: Int) -> (Array[UInt64], BytesView) raise DecodeError {
  any_rle_take_n_finalize(bytes, n, read_postcard_usize)
}

pub fn encode_any_rle_usize(values: Array[UInt64]) -> Bytes {
  encode_any_rle_literal(values, write_postcard_usize)
}

pub fn decode_any_rle_u32(bytes: BytesView) -> Array[UInt] raise DecodeError {
  decode_any_rle(bytes, read_postcard_u32)
}

pub fn any_rle_take_n_finalize_u32(bytes: BytesView, n: Int) -> (Array[UInt], BytesView) raise DecodeError {
  any_rle_take_n_finalize(bytes, n, read_postcard_u32)
}

pub fn encode_any_rle_u32(values: Array[UInt]) -> Bytes {
  encode_any_rle_literal(values, write_postcard_u32)
}

pub fn decode_any_rle_u64(bytes: BytesView) -> Array[UInt64] raise DecodeError {
  decode_any_rle(bytes, read_postcard_u64)
}

pub fn encode_any_rle_u64(values: Array[UInt64]) -> Bytes {
  encode_any_rle_literal(values, write_postcard_u64)
}

pub fn decode_any_rle_i32(bytes: BytesView) -> Array[Int] raise DecodeError {
  decode_any_rle(bytes, read_postcard_i32)
}

pub fn any_rle_take_n_finalize_i32(bytes: BytesView, n: Int) -> (Array[Int], BytesView) raise DecodeError {
  any_rle_take_n_finalize(bytes, n, read_postcard_i32)
}

pub fn encode_any_rle_i32(values: Array[Int]) -> Bytes {
  encode_any_rle_literal(values, write_postcard_i32)
}

fn read_varint_u128_bigint(r: BytesReader) -> BigInt raise DecodeError {
  let mut result: BigInt = 0N
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

fn write_varint_u128_bigint(w: BytesWriter, value: BigInt) -> Unit {
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

fn zigzag_decode_bigint(encoded: BigInt) -> BigInt {
  if (encoded & 1N) == 0N {
    encoded >> 1
  } else {
    -((encoded >> 1) + 1N)
  }
}

fn zigzag_encode_bigint(value: BigInt) -> BigInt {
  if value >= 0N {
    value << 1
  } else {
    (-value << 1) - 1N
  }
}

fn read_postcard_i128_bigint(r: BytesReader) -> BigInt raise DecodeError {
  zigzag_decode_bigint(read_varint_u128_bigint(r))
}

fn write_postcard_i128_bigint(w: BytesWriter, v: BigInt) -> Unit {
  write_varint_u128_bigint(w, zigzag_encode_bigint(v))
}

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

pub fn decode_delta_of_delta_i64(bytes: BytesView) -> Array[Int64] raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let first_opt = read_postcard_option_i64(r)
  if r.remaining() < 1 {
    raise DecodeError("delta_of_delta: invalid bytes")
  }
  let last_used_bits = r.read_u8().to_uint()
  let bitstream = r.read_exact(r.remaining())
  match first_opt {
    None => {
      if last_used_bits != 0 || bitstream.length() != 0 {
        raise DecodeError("delta_of_delta: invalid empty encoding")
      }
      []
    }
    Some(first) => {
      let br = BitReader::new(bitstream, last_used_bits)
      let out: Array[Int64] = [first]
      let mut prev = first
      let mut delta: Int64 = 0
      while br.remaining() > 0 {
        let dod = decode_delta_of_delta_value(br)
        delta = delta + dod
        prev = prev + delta
        out.push(prev)
      }
      out
    }
  }
}

priv struct DeltaOfDeltaDecoder {
  bits: BytesView
  mut head_num: Int64?
  mut prev_value: Int64
  mut prev_delta: Int64
  mut index: Int
  mut current_bits_index: Int
  last_used_bits: Int
}

fn DeltaOfDeltaDecoder::new(
  bits: BytesView,
  head_num: Int64?,
  last_used_bits: UInt,
) -> DeltaOfDeltaDecoder raise DecodeError {
  if last_used_bits > 8 {
    raise DecodeError("delta_of_delta: invalid last_used_bits")
  }
  { bits, head_num, prev_value: 0L, prev_delta: 0L, index: 0, current_bits_index: 0, last_used_bits: last_used_bits.reinterpret_as_int() }
}

fn DeltaOfDeltaDecoder::read_bits(self: DeltaOfDeltaDecoder, count: Int) -> UInt64? {
  if self.index >= self.bits.length() {
    return None
  }
  let total_bits = if self.bits.length() == 0 {
    0
  } else {
    (self.bits.length() - 1) * 8 + self.last_used_bits
  }
  let read_bits = self.index * 8 + self.current_bits_index
  let remaining_bits = total_bits - read_bits
  if remaining_bits < count {
    return None
  }

  let current_byte_remaining = 8 - self.current_bits_index
  if count <= current_byte_remaining {
    let current_index = self.index
    self.current_bits_index = self.current_bits_index + count
    if self.current_bits_index == 8 {
      self.index = self.index + 1
      self.current_bits_index = 0
    }
    let mask = 0xFF >> (8 - count)
    let current_byte = self.bits[current_index].to_uint()
    let after_shift = current_byte >> (current_byte_remaining - count)
    Some((after_shift & mask.reinterpret_as_uint()).to_uint64())
  } else {
    let mut ans: UInt64 = (self.bits[self.index].to_uint() & (0xFF >> (8 - current_byte_remaining))).to_uint64()
    self.index = self.index + 1
    self.current_bits_index = 0
    let mut rest = count - current_byte_remaining
    while rest > 8 {
      ans = (ans << 8) | self.bits[self.index].to_uint64()
      self.index = self.index + 1
      rest = rest - 8
    }
    ans = (ans << rest) | (self.bits[self.index].to_uint() >> (8 - rest)).to_uint64()
    self.current_bits_index = self.current_bits_index + rest
    if self.current_bits_index == 8 {
      self.index = self.index + 1
      self.current_bits_index = 0
    }
    Some(ans)
  }
}

fn DeltaOfDeltaDecoder::try_next(self: DeltaOfDeltaDecoder) -> Int64? raise DecodeError {
  match self.head_num {
    Some(head) => {
      self.prev_value = head
      self.head_num = None
    }
    None => {
      match self.read_bits(1) {
        None => return None
        Some(0UL) => {
          self.prev_value = self.prev_value + self.prev_delta
        }
        Some(1UL) => {
          let mut num_bits = 0
          let mut bias: Int64 = 0
          match self.read_bits(1) {
            Some(0UL) => { num_bits = 7; bias = 63L }
            Some(1UL) => {
              match self.read_bits(1) {
                Some(0UL) => { num_bits = 9; bias = 255L }
                Some(1UL) => {
                  match self.read_bits(1) {
                    Some(0UL) => { num_bits = 12; bias = 2047L }
                    Some(1UL) => {
                      match self.read_bits(1) {
                        Some(0UL) => { num_bits = 21; bias = 1048575L }
                        Some(1UL) => { num_bits = 64; bias = 0L }
                        _ => raise DecodeError("delta_of_delta: invalid flag")
                      }
                    }
                    _ => raise DecodeError("delta_of_delta: invalid flag")
                  }
                }
                _ => raise DecodeError("delta_of_delta: invalid flag")
              }
            }
            _ => raise DecodeError("delta_of_delta: invalid flag")
          }
          match self.read_bits(num_bits) {
            Some(bits) => {
              let delta_of_delta = bits.reinterpret_as_int64() - bias
              self.prev_delta = self.prev_delta + delta_of_delta
              self.prev_value = self.prev_value + self.prev_delta
            }
            None => return None
          }
        }
        _ => raise DecodeError("delta_of_delta: invalid flag")
      }
    }
  }
  Some(self.prev_value)
}

fn DeltaOfDeltaDecoder::finalize(self: DeltaOfDeltaDecoder) -> BytesView {
  if self.bits.length() == 0 {
    return self.bits
  }
  if self.current_bits_index > 0 {
    self.index = self.index + 1
  }
  if self.index < 0 {
    return self.bits
  }
  if self.index > self.bits.length() {
    return self.bits[self.bits.length():self.bits.length()]
  }
  self.bits[self.index:self.bits.length()]
}

pub fn delta_of_delta_take_n_finalize_i64(bytes: BytesView, n: Int) -> (Array[Int64], BytesView) raise DecodeError {
  if n < 0 {
    raise DecodeError("delta_of_delta: invalid n")
  }
  let r = BytesReader::from_view(bytes)
  let first_opt = read_postcard_option_i64(r)
  if r.remaining() < 1 {
    raise DecodeError("delta_of_delta: invalid bytes")
  }
  let last_used_bits = r.read_u8().to_uint()
  let bits = r.remaining_view()
  match first_opt {
    None => {
      if n != 0 {
        raise DecodeError("delta_of_delta: not enough elements")
      }
      if last_used_bits != 0 {
        raise DecodeError("delta_of_delta: invalid empty encoding")
      }
      ([], bits)
    }
    Some(_) => {
      if n == 0 {
        raise DecodeError("delta_of_delta: too many elements")
      }
      let dec = DeltaOfDeltaDecoder::new(bits, first_opt, last_used_bits)
      let out: Array[Int64] = []
      for _i in 0..<n {
        match dec.try_next() {
          None => raise DecodeError("delta_of_delta: not enough elements")
          Some(v) => out.push(v)
        }
      }
      (out, dec.finalize())
    }
  }
}

priv struct BitWriter {
  bytes: Array[Byte]
  mut cur: UInt
  mut bits: Int
}

fn BitWriter::new() -> BitWriter {
  { bytes: [], cur: 0, bits: 0 }
}

fn BitWriter::write_bit(self: BitWriter, bit: Bool) -> Unit {
  self.cur = (self.cur << 1) | (if bit { 1 } else { 0 })
  self.bits = self.bits + 1
  if self.bits == 8 {
    self.bytes.push((self.cur & 0xFF).to_byte())
    self.cur = 0
    self.bits = 0
  }
}

fn BitWriter::write_bits_u64(self: BitWriter, v: UInt64, n: Int) -> Unit {
  for i in 0..<n {
    let shift = n - 1 - i
    let bit = ((v >> shift) & 1UL) != 0UL
    self.write_bit(bit)
  }
}

fn encode_delta_of_delta_value(bw: BitWriter, v: Int64) -> Unit {
  if v == 0 {
    bw.write_bit(false)
    return
  }
  if v >= -63L && v <= 64L {
    bw.write_bits_u64(0b10UL, 2)
    bw.write_bits_u64((v + 63L).reinterpret_as_uint64(), 7)
    return
  }
  if v >= -255L && v <= 256L {
    bw.write_bits_u64(0b110UL, 3)
    bw.write_bits_u64((v + 255L).reinterpret_as_uint64(), 9)
    return
  }
  if v >= -2047L && v <= 2048L {
    bw.write_bits_u64(0b1110UL, 4)
    bw.write_bits_u64((v + 2047L).reinterpret_as_uint64(), 12)
    return
  }
  if v >= -1048575L && v <= 1048576L {
    bw.write_bits_u64(0b11110UL, 5)
    bw.write_bits_u64((v + 1048575L).reinterpret_as_uint64(), 21)
    return
  }
  bw.write_bits_u64(0b11111UL, 5)
  bw.write_bits_u64(v.reinterpret_as_uint64(), 64)
}

pub fn encode_delta_of_delta_i64(values: Array[Int64]) -> Bytes {
  let w = BytesWriter::new()
  if values.length() == 0 {
    write_postcard_option_i64(w, None)
    w.write_u8(b'\x00')
    return w.to_bytes()
  }

  write_postcard_option_i64(w, Some(values[0]))
  if values.length() == 1 {
    w.write_u8(b'\x00')
    return w.to_bytes()
  }

  let bw = BitWriter::new()
  let mut prev_delta: Int64 = 0
  for i in 1..<values.length() {
    let d = values[i] - values[i - 1]
    let dod = d - prev_delta
    prev_delta = d
    encode_delta_of_delta_value(bw, dod)
  }

  if bw.bits == 0 {
    w.write_u8(b'\x08')
    w.write_bytes(Bytes::from_array(bw.bytes))
    return w.to_bytes()
  }
  let last_used_bits = bw.bits
  let last = ((bw.cur & 0xFF) << (8 - last_used_bits)).to_byte()
  bw.bytes.push(last)
  w.write_u8(last_used_bits.to_byte())
  w.write_bytes(Bytes::from_array(bw.bytes))
  w.to_bytes()
}
