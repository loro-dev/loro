///|
pub fn decode_value(bytes : BytesView) -> (Value, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let tag = r.read_u8()
  decode_value_content(tag, bytes[1:])
}

///|
pub fn decode_value_content(
  tag : Byte,
  bytes : BytesView,
) -> (Value, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let kind = tag.to_uint() & 0x7F
  let v : Value = match kind {
    0 => Value::Null
    1 => Value::True
    2 => Value::False
    3 => Value::I64(r.read_sleb128_i64())
    4 => {
      let bits = r.read_u64_be()
      Value::F64(bits.reinterpret_as_double())
    }
    5 => Value::Str(read_utf8(r))
    6 => Value::Binary(read_binary(r))
    7 => Value::ContainerIdx(r.read_uleb128_u64())
    8 => Value::DeleteOnce
    9 => Value::DeleteSeq
    10 => {
      let x = r.read_sleb128_i64()
      if x < -2147483648L || x > 2147483647L {
        raise DecodeError("value: deltaint overflow")
      }
      Value::DeltaInt(x.to_int())
    }
    11 => Value::LoroValue(read_loro_value(r, 0))
    12 => {
      let info = r.read_u8()
      let len = r.read_uleb128_u64()
      let key_idx = r.read_uleb128_u64()
      let value = read_loro_value(r, 0)
      Value::MarkStart({ info, len, key_idx, value })
    }
    13 => {
      let target_idx = r.read_uleb128_u64()
      let is_parent_null = r.read_u8() != b'\x00'
      let position = r.read_uleb128_u64()
      let parent_idx = if is_parent_null {
        Option::None
      } else {
        Option::Some(r.read_uleb128_u64())
      }
      Value::TreeMove({ target_idx, is_parent_null, position, parent_idx })
    }
    14 => {
      let from = r.read_uleb128_u64()
      let from_idx = r.read_uleb128_u64()
      let lamport = r.read_uleb128_u64()
      Value::ListMove({ from, from_idx, lamport })
    }
    15 => {
      let peer_idx = r.read_uleb128_u64()
      let lamport_u64 = r.read_uleb128_u64()
      if lamport_u64 > 0xFFFF_FFFFUL {
        raise DecodeError("value: listset lamport overflow")
      }
      let value = read_loro_value(r, 0)
      Value::ListSet({ peer_idx, lamport: lamport_u64.to_uint(), value })
    }
    16 => {
      let subject_peer_idx = r.read_uleb128_u64()
      let subject_cnt_u64 = r.read_uleb128_u64()
      let position_idx = r.read_uleb128_u64()
      let is_parent_null = r.read_u8() != b'\x00'
      let mut parent_peer_idx : UInt64 = 0
      let mut parent_cnt_u64 : UInt64 = 0
      if !is_parent_null {
        parent_peer_idx = r.read_uleb128_u64()
        parent_cnt_u64 = r.read_uleb128_u64()
      }
      if subject_cnt_u64 > 0x7FFF_FFFFUL || parent_cnt_u64 > 0x7FFF_FFFFUL {
        raise DecodeError("value: raw_tree_move counter overflow")
      }
      Value::RawTreeMove({
        subject_peer_idx,
        subject_cnt: subject_cnt_u64.to_int(),
        position_idx,
        is_parent_null,
        parent_peer_idx,
        parent_cnt: parent_cnt_u64.to_int(),
      })
    }
    _ => {
      let data = read_binary(r)
      Value::Future(tag, data)
    }
  }
  let rest = bytes[bytes.length() - r.remaining():]
  (v, rest)
}
