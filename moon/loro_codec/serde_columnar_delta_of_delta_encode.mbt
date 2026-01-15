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
