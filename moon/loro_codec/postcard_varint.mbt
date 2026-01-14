// Postcard 的整数编码基于 varint + zigzag（不是 SLEB128）。

pub fn BytesReader::read_varint_u64(self : BytesReader) -> UInt64 raise DecodeError {
  let mut result: UInt64 = 0
  let mut shift = 0
  // u64 varint 最多 10 字节
  for _i in 0..<10 {
    let byte = self.read_u8().to_uint64()
    result = result | ((byte & 0x7F) << shift)
    if (byte & 0x80) == 0 {
      return result
    }
    shift = shift + 7
  }
  raise DecodeError("varint too long")
}

pub fn BytesWriter::write_varint_u64(self : BytesWriter, value : UInt64) -> Unit {
  let mut value = value
  while true {
    let mut byte = value & 0x7F
    value = value >> 7
    if value != 0 {
      byte = byte | 0x80
    }
    self.write_u8(byte.to_byte())
    if value == 0 {
      break
    }
  }
}

pub fn zigzag_encode_i64(value: Int64) -> UInt64 {
  ((value << 1) ^ (value >> 63)).reinterpret_as_uint64()
}

pub fn zigzag_decode_i64(value: UInt64) -> Int64 {
  let lo = (value & 1).reinterpret_as_int64()
  (value >> 1).reinterpret_as_int64() ^ (-lo)
}

pub fn BytesReader::read_varint_i64(self : BytesReader) -> Int64 raise DecodeError {
  zigzag_decode_i64(self.read_varint_u64())
}

pub fn BytesWriter::write_varint_i64(self : BytesWriter, value: Int64) -> Unit {
  self.write_varint_u64(zigzag_encode_i64(value))
}
