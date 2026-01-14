pub fn BytesReader::read_uleb128_u64(self : BytesReader) -> UInt64 raise DecodeError {
  let mut result: UInt64 = 0
  let mut shift = 0
  while true {
    if shift >= 64 {
      raise DecodeError("uleb128 overflow")
    }
    let byte = self.read_u8().to_uint64()
    result = result | ((byte & 0x7F) << shift)
    if (byte & 0x80) == 0 {
      return result
    }
    shift = shift + 7
  }
  0
}

pub fn BytesWriter::write_uleb128_u64(self : BytesWriter, value : UInt64) -> Unit {
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

pub fn BytesReader::read_sleb128_i64(self : BytesReader) -> Int64 raise DecodeError {
  let mut result: Int64 = 0
  let mut shift = 0
  let mut byte: UInt64 = 0
  while true {
    if shift >= 64 {
      raise DecodeError("sleb128 overflow")
    }
    byte = self.read_u8().to_uint64()
    let slice = (byte & 0x7F).reinterpret_as_int64()
    result = result | (slice << shift)
    shift = shift + 7
    if (byte & 0x80) == 0 {
      break
    }
  }
  if shift < 64 && (byte & 0x40) != 0 {
    result = result | (-1 << shift)
  }
  result
}

pub fn BytesWriter::write_sleb128_i64(self : BytesWriter, value : Int64) -> Unit {
  let mut value = value
  let mut more = true
  while more {
    let mut byte = (value & 0x7F).to_byte()
    value = value >> 7
    let sign_bit = (byte & 0x40) != 0
    if (value == 0 && !sign_bit) || (value == -1 && sign_bit) {
      more = false
    } else {
      byte = byte | 0x80
    }
    self.write_u8(byte)
  }
}
