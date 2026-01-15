///|
fn[T] decode_any_rle(
  bytes : BytesView,
  read_value : (BytesReader) -> T raise DecodeError,
) -> Array[T] raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let out : Array[T] = []
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

///|
fn[T] any_rle_take_n_finalize(
  bytes : BytesView,
  n : Int,
  read_value : (BytesReader) -> T raise DecodeError,
) -> (Array[T], BytesView) raise DecodeError {
  if n < 0 {
    raise DecodeError("anyrle: invalid n")
  }
  if n == 0 {
    return ([], bytes)
  }
  let r = BytesReader::from_view(bytes)
  let out : Array[T] = []
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

///|
fn[T] encode_any_rle_literal(
  values : Array[T],
  write_value : (BytesWriter, T) -> Unit,
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

///|
fn read_postcard_u8(r : BytesReader) -> UInt raise DecodeError {
  r.read_u8().to_uint()
}

///|
fn write_postcard_u8(w : BytesWriter, v : UInt) -> Unit {
  w.write_u8((v & 0xFF).to_byte())
}

///|
fn read_postcard_u32(r : BytesReader) -> UInt raise DecodeError {
  let v = r.read_varint_u64()
  if v > 0xFFFF_FFFFUL {
    raise DecodeError("postcard: u32 overflow")
  }
  v.to_uint()
}

///|
fn write_postcard_u32(w : BytesWriter, v : UInt) -> Unit {
  w.write_varint_u64(v.to_uint64())
}

///|
fn read_postcard_u64(r : BytesReader) -> UInt64 raise DecodeError {
  r.read_varint_u64()
}

///|
fn write_postcard_u64(w : BytesWriter, v : UInt64) -> Unit {
  w.write_varint_u64(v)
}

///|
fn read_postcard_usize(r : BytesReader) -> UInt64 raise DecodeError {
  r.read_varint_u64()
}

///|
fn write_postcard_usize(w : BytesWriter, v : UInt64) -> Unit {
  w.write_varint_u64(v)
}

///|
fn read_postcard_i32(r : BytesReader) -> Int raise DecodeError {
  let v = r.read_varint_i64()
  if v < -2147483648L || v > 2147483647L {
    raise DecodeError("postcard: i32 overflow")
  }
  v.to_int()
}

///|
fn write_postcard_i32(w : BytesWriter, v : Int) -> Unit {
  w.write_varint_i64(v.to_int64())
}
