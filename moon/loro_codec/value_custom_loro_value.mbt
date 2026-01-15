///|
fn read_loro_value(r : BytesReader, depth : Int) -> LoroValue raise DecodeError {
  if depth > 1024 {
    raise DecodeError("value: too deep")
  }
  let kind = r.read_u8().to_uint()
  match kind {
    0 => LoroValue::Null
    1 => LoroValue::True
    2 => LoroValue::False
    3 => LoroValue::I64(r.read_sleb128_i64())
    4 => {
      let bits = r.read_u64_be()
      LoroValue::F64(bits.reinterpret_as_double())
    }
    5 => LoroValue::Str(read_utf8(r))
    6 => LoroValue::Binary(read_binary(r))
    7 => {
      let len = read_len_u64(r).to_int()
      let items : Array[LoroValue] = []
      for _i in 0..<len {
        items.push(read_loro_value(r, depth + 1))
      }
      LoroValue::List(items)
    }
    8 => {
      let len = read_len_u64(r).to_int()
      let items : Array[(UInt64, LoroValue)] = []
      for _i in 0..<len {
        let key_idx = r.read_uleb128_u64()
        let v = read_loro_value(r, depth + 1)
        items.push((key_idx, v))
      }
      LoroValue::Map(items)
    }
    9 => {
      let ct = r.read_u8()
      LoroValue::ContainerType(ct)
    }
    _ => raise DecodeError("value: invalid loro_value kind")
  }
}

///|
fn write_loro_value(w : BytesWriter, v : LoroValue) -> Unit {
  match v {
    LoroValue::Null => w.write_u8(b'\x00')
    LoroValue::True => w.write_u8(b'\x01')
    LoroValue::False => w.write_u8(b'\x02')
    LoroValue::I64(x) => {
      w.write_u8(b'\x03')
      w.write_sleb128_i64(x)
    }
    LoroValue::F64(x) => {
      w.write_u8(b'\x04')
      w.write_u64_be(x.reinterpret_as_uint64())
    }
    LoroValue::Str(s) => {
      w.write_u8(b'\x05')
      write_utf8(w, s)
    }
    LoroValue::Binary(b) => {
      w.write_u8(b'\x06')
      write_binary(w, b)
    }
    LoroValue::List(items) => {
      w.write_u8(b'\x07')
      w.write_uleb128_u64(items.length().to_uint64())
      for it in items {
        write_loro_value(w, it)
      }
    }
    LoroValue::Map(items) => {
      w.write_u8(b'\x08')
      w.write_uleb128_u64(items.length().to_uint64())
      for pair in items {
        let (k, it) = pair
        w.write_uleb128_u64(k)
        write_loro_value(w, it)
      }
    }
    LoroValue::ContainerType(ct) => {
      w.write_u8(b'\x09')
      w.write_u8(ct)
    }
  }
}
