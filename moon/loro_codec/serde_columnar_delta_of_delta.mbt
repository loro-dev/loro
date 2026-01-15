///|
pub fn decode_delta_of_delta_i64(
  bytes : BytesView,
) -> Array[Int64] raise DecodeError {
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
      let out : Array[Int64] = [first]
      let mut prev = first
      let mut delta : Int64 = 0
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

///|
priv struct DeltaOfDeltaDecoder {
  bits : BytesView
  mut head_num : Int64?
  mut prev_value : Int64
  mut prev_delta : Int64
  mut index : Int
  mut current_bits_index : Int
  last_used_bits : Int
}

///|
fn DeltaOfDeltaDecoder::new(
  bits : BytesView,
  head_num : Int64?,
  last_used_bits : UInt,
) -> DeltaOfDeltaDecoder raise DecodeError {
  if last_used_bits > 8 {
    raise DecodeError("delta_of_delta: invalid last_used_bits")
  }
  {
    bits,
    head_num,
    prev_value: 0L,
    prev_delta: 0L,
    index: 0,
    current_bits_index: 0,
    last_used_bits: last_used_bits.reinterpret_as_int(),
  }
}

///|
fn DeltaOfDeltaDecoder::read_bits(
  self : DeltaOfDeltaDecoder,
  count : Int,
) -> UInt64? {
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
    let mut ans : UInt64 = (self.bits[self.index].to_uint() &
    (0xFF >> (8 - current_byte_remaining))).to_uint64()
    self.index = self.index + 1
    self.current_bits_index = 0
    let mut rest = count - current_byte_remaining
    while rest > 8 {
      ans = (ans << 8) | self.bits[self.index].to_uint64()
      self.index = self.index + 1
      rest = rest - 8
    }
    ans = (ans << rest) |
      (self.bits[self.index].to_uint() >> (8 - rest)).to_uint64()
    self.current_bits_index = self.current_bits_index + rest
    if self.current_bits_index == 8 {
      self.index = self.index + 1
      self.current_bits_index = 0
    }
    Some(ans)
  }
}

///|
fn DeltaOfDeltaDecoder::try_next(
  self : DeltaOfDeltaDecoder,
) -> Int64? raise DecodeError {
  match self.head_num {
    Some(head) => {
      self.prev_value = head
      self.head_num = None
    }
    None =>
      match self.read_bits(1) {
        None => return None
        Some(0UL) => self.prev_value = self.prev_value + self.prev_delta
        Some(1UL) => {
          let mut num_bits = 0
          let mut bias : Int64 = 0
          match self.read_bits(1) {
            Some(0UL) => {
              num_bits = 7
              bias = 63L
            }
            Some(1UL) =>
              match self.read_bits(1) {
                Some(0UL) => {
                  num_bits = 9
                  bias = 255L
                }
                Some(1UL) =>
                  match self.read_bits(1) {
                    Some(0UL) => {
                      num_bits = 12
                      bias = 2047L
                    }
                    Some(1UL) =>
                      match self.read_bits(1) {
                        Some(0UL) => {
                          num_bits = 21
                          bias = 1048575L
                        }
                        Some(1UL) => {
                          num_bits = 64
                          bias = 0L
                        }
                        _ => raise DecodeError("delta_of_delta: invalid flag")
                      }
                    _ => raise DecodeError("delta_of_delta: invalid flag")
                  }
                _ => raise DecodeError("delta_of_delta: invalid flag")
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
  Some(self.prev_value)
}

///|
fn DeltaOfDeltaDecoder::finalize(self : DeltaOfDeltaDecoder) -> BytesView {
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

///|
pub fn delta_of_delta_take_n_finalize_i64(
  bytes : BytesView,
  n : Int,
) -> (Array[Int64], BytesView) raise DecodeError {
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
      let out : Array[Int64] = []
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

///|
priv struct BitWriter {
  bytes : Array[Byte]
  mut cur : UInt
  mut bits : Int
}

///|
fn BitWriter::new() -> BitWriter {
  { bytes: [], cur: 0, bits: 0 }
}

///|
fn BitWriter::write_bit(self : BitWriter, bit : Bool) -> Unit {
  self.cur = (self.cur << 1) | (if bit { 1 } else { 0 })
  self.bits = self.bits + 1
  if self.bits == 8 {
    self.bytes.push((self.cur & 0xFF).to_byte())
    self.cur = 0
    self.bits = 0
  }
}

///|
fn BitWriter::write_bits_u64(self : BitWriter, v : UInt64, n : Int) -> Unit {
  for i in 0..<n {
    let shift = n - 1 - i
    let bit = ((v >> shift) & 1UL) != 0UL
    self.write_bit(bit)
  }
}

///|
fn encode_delta_of_delta_value(bw : BitWriter, v : Int64) -> Unit {
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

///|
pub fn encode_delta_of_delta_i64(values : Array[Int64]) -> Bytes {
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
  let mut prev_delta : Int64 = 0
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
