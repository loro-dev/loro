pub struct BytesReader {
  buf: BytesView
  mut pos: Int
}

pub fn BytesReader::new(bytes: Bytes) -> BytesReader {
  { buf: bytes[:], pos: 0 }
}

pub fn BytesReader::from_view(view: BytesView) -> BytesReader {
  { buf: view, pos: 0 }
}

pub fn BytesReader::remaining(self: BytesReader) -> Int {
  self.buf.length() - self.pos
}

fn BytesReader::require(self: BytesReader, n: Int) -> Unit raise DecodeError {
  if n < 0 {
    raise DecodeError("invalid read length")
  }
  if self.pos + n > self.buf.length() {
    raise DecodeError("unexpected eof")
  }
}

pub fn BytesReader::read_u8(self: BytesReader) -> Byte raise DecodeError {
  self.require(1)
  let b = self.buf[self.pos]
  self.pos = self.pos + 1
  b
}

pub fn BytesReader::read_exact(self: BytesReader, n: Int) -> BytesView raise DecodeError {
  self.require(n)
  let start = self.pos
  self.pos = self.pos + n
  self.buf[start:start + n]
}

pub fn BytesReader::skip(self: BytesReader, n: Int) -> Unit raise DecodeError {
  let _ = self.read_exact(n)
}

pub fn BytesReader::read_u16_le(self: BytesReader) -> UInt raise DecodeError {
  let b0 = self.read_u8().to_uint()
  let b1 = self.read_u8().to_uint()
  b0 | (b1 << 8)
}

pub fn BytesReader::read_u16_be(self: BytesReader) -> UInt raise DecodeError {
  let b0 = self.read_u8().to_uint()
  let b1 = self.read_u8().to_uint()
  (b0 << 8) | b1
}

pub fn BytesReader::read_u32_le(self: BytesReader) -> UInt raise DecodeError {
  let b0 = self.read_u8().to_uint()
  let b1 = self.read_u8().to_uint()
  let b2 = self.read_u8().to_uint()
  let b3 = self.read_u8().to_uint()
  b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
}

pub fn BytesReader::read_u32_be(self: BytesReader) -> UInt raise DecodeError {
  let b0 = self.read_u8().to_uint()
  let b1 = self.read_u8().to_uint()
  let b2 = self.read_u8().to_uint()
  let b3 = self.read_u8().to_uint()
  (b0 << 24) | (b1 << 16) | (b2 << 8) | b3
}

pub fn BytesReader::read_u64_le(self: BytesReader) -> UInt64 raise DecodeError {
  let mut v: UInt64 = 0
  for i in 0..<8 {
    let b = self.read_u8().to_uint64()
    v = v | (b << (8 * i))
  }
  v
}

pub fn BytesReader::read_u64_be(self: BytesReader) -> UInt64 raise DecodeError {
  let mut v: UInt64 = 0
  for i in 0..<8 {
    let b = self.read_u8().to_uint64()
    v = (v << 8) | b
  }
  v
}

pub struct BytesWriter {
  buf: @buffer.Buffer
}

pub fn BytesWriter::new() -> BytesWriter {
  { buf: @buffer.new() }
}

pub fn BytesWriter::to_bytes(self: BytesWriter) -> Bytes {
  self.buf.to_bytes()
}

pub fn BytesWriter::write_u8(self: BytesWriter, value: Byte) -> Unit {
  self.buf.write_byte(value)
}

pub fn BytesWriter::write_bytes(self: BytesWriter, value: Bytes) -> Unit {
  self.buf.write_bytes(value)
}

pub fn BytesWriter::write_bytesview(self: BytesWriter, value: BytesView) -> Unit {
  self.buf.write_bytesview(value)
}

pub fn BytesWriter::write_u16_le(self: BytesWriter, value: UInt) -> Unit {
  self.write_u8((value & 0xFF).to_byte())
  self.write_u8(((value >> 8) & 0xFF).to_byte())
}

pub fn BytesWriter::write_u16_be(self: BytesWriter, value: UInt) -> Unit {
  self.write_u8(((value >> 8) & 0xFF).to_byte())
  self.write_u8((value & 0xFF).to_byte())
}

pub fn BytesWriter::write_u32_le(self: BytesWriter, value: UInt) -> Unit {
  self.write_u8((value & 0xFF).to_byte())
  self.write_u8(((value >> 8) & 0xFF).to_byte())
  self.write_u8(((value >> 16) & 0xFF).to_byte())
  self.write_u8(((value >> 24) & 0xFF).to_byte())
}

pub fn BytesWriter::write_u32_be(self: BytesWriter, value: UInt) -> Unit {
  self.write_u8(((value >> 24) & 0xFF).to_byte())
  self.write_u8(((value >> 16) & 0xFF).to_byte())
  self.write_u8(((value >> 8) & 0xFF).to_byte())
  self.write_u8((value & 0xFF).to_byte())
}

pub fn BytesWriter::write_u64_le(self: BytesWriter, value: UInt64) -> Unit {
  let mut value = value
  for _i in 0..<8 {
    self.write_u8((value & 0xFF).to_byte())
    value = value >> 8
  }
}

pub fn BytesWriter::write_u64_be(self: BytesWriter, value: UInt64) -> Unit {
  for i in 0..<8 {
    let shift = 56 - (8 * i)
    self.write_u8(((value >> shift) & 0xFF).to_byte())
  }
}
