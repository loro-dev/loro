///|
pub fn encode_value_content(v : Value) -> (Byte, Bytes) {
  let w = BytesWriter::new()
  match v {
    Value::Null => (b'\x00', b"")
    Value::True => (b'\x01', b"")
    Value::False => (b'\x02', b"")
    Value::I64(x) => {
      w.write_sleb128_i64(x)
      (b'\x03', w.to_bytes())
    }
    Value::F64(x) => {
      w.write_u64_be(x.reinterpret_as_uint64())
      (b'\x04', w.to_bytes())
    }
    Value::Str(s) => {
      write_utf8(w, s)
      (b'\x05', w.to_bytes())
    }
    Value::Binary(b) => {
      write_binary(w, b)
      (b'\x06', w.to_bytes())
    }
    Value::ContainerIdx(idx) => {
      w.write_uleb128_u64(idx)
      (b'\x07', w.to_bytes())
    }
    Value::DeleteOnce => (b'\x08', b"")
    Value::DeleteSeq => (b'\x09', b"")
    Value::DeltaInt(x) => {
      w.write_sleb128_i64(x.to_int64())
      (b'\x0A', w.to_bytes())
    }
    Value::LoroValue(v) => {
      write_loro_value(w, v)
      (b'\x0B', w.to_bytes())
    }
    Value::MarkStart(m) => {
      w.write_u8(m.info)
      w.write_uleb128_u64(m.len)
      w.write_uleb128_u64(m.key_idx)
      write_loro_value(w, m.value)
      (b'\x0C', w.to_bytes())
    }
    Value::TreeMove(t) => {
      w.write_uleb128_u64(t.target_idx)
      w.write_u8(if t.is_parent_null { b'\x01' } else { b'\x00' })
      w.write_uleb128_u64(t.position)
      match t.parent_idx {
        Option::None => ()
        Option::Some(p) => w.write_uleb128_u64(p)
      }
      (b'\x0D', w.to_bytes())
    }
    Value::ListMove(m) => {
      w.write_uleb128_u64(m.from)
      w.write_uleb128_u64(m.from_idx)
      w.write_uleb128_u64(m.lamport)
      (b'\x0E', w.to_bytes())
    }
    Value::ListSet(s) => {
      w.write_uleb128_u64(s.peer_idx)
      w.write_uleb128_u64(s.lamport.to_uint64())
      write_loro_value(w, s.value)
      (b'\x0F', w.to_bytes())
    }
    Value::RawTreeMove(t) => {
      w.write_uleb128_u64(t.subject_peer_idx)
      w.write_uleb128_u64(t.subject_cnt.to_uint64())
      w.write_uleb128_u64(t.position_idx)
      w.write_u8(if t.is_parent_null { b'\x01' } else { b'\x00' })
      if !t.is_parent_null {
        w.write_uleb128_u64(t.parent_peer_idx)
        w.write_uleb128_u64(t.parent_cnt.to_uint64())
      }
      (b'\x10', w.to_bytes())
    }
    Value::Future(tag, data) => {
      write_binary(w, data)
      (tag, w.to_bytes())
    }
  }
}

///|
pub fn encode_value(v : Value) -> Bytes {
  let w = BytesWriter::new()
  let (tag, content) = encode_value_content(v)
  w.write_u8(tag)
  w.write_bytes(content)
  w.to_bytes()
}
