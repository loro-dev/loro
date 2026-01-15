pub fn decode_keys(bytes: BytesView) -> Array[String] raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let out: Array[String] = []
  while r.remaining() > 0 {
    let len_u64 = r.read_uleb128_u64()
    if len_u64 > 0x7FFF_FFFFUL {
      raise DecodeError("keys: key too long")
    }
    let len = len_u64.to_int()
    if len < 0 || len > r.remaining() {
      raise DecodeError("keys: invalid key length")
    }
    let key_bytes = r.read_exact(len)
    let key = @encoding/utf8.decode(key_bytes) catch { @encoding/utf8.Malformed(_) =>
      raise DecodeError("keys: invalid utf8")
    }
    out.push(key)
  }
  out
}

pub fn decode_container_arena(
  bytes: BytesView,
  peers: Array[UInt64],
  keys: Array[String],
) -> Array[ContainerID] raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let n_u64 = r.read_varint_u64()
  if n_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("container_arena: too many containers")
  }
  let n = n_u64.to_int()
  if n < 0 {
    raise DecodeError("container_arena: invalid len")
  }

  let out: Array[ContainerID] = []
  for _i in 0..<n {
    // EncodedContainer uses serde to serialize as a seq of 4 fields.
    let field_len = r.read_varint_u64()
    if field_len != 4UL {
      raise DecodeError("container_arena: invalid field length")
    }
    let is_root = match r.read_u8() {
      b'\x00' => false
      b'\x01' => true
      _ => raise DecodeError("container_arena: invalid bool")
    }
    let kind = container_type_from_u8(r.read_u8())
    let peer_idx_u64 = r.read_varint_u64()
    if peer_idx_u64 > 0x7FFF_FFFFUL {
      raise DecodeError("container_arena: peer_idx too large")
    }
    let peer_idx = peer_idx_u64.to_int()
    let key_idx_or_counter_i64 = r.read_varint_i64()
    if key_idx_or_counter_i64 < -2147483648L || key_idx_or_counter_i64 > 2147483647L {
      raise DecodeError("container_arena: i32 overflow")
    }
    let key_idx_or_counter = key_idx_or_counter_i64.to_int()
    if is_root {
      if key_idx_or_counter < 0 || key_idx_or_counter >= keys.length() {
        raise DecodeError("container_arena: invalid root key idx")
      }
      out.push(ContainerID::root(keys[key_idx_or_counter], kind))
    } else {
      if peer_idx < 0 || peer_idx >= peers.length() {
        raise DecodeError("container_arena: invalid peer idx")
      }
      out.push(ContainerID::normal(peers[peer_idx], key_idx_or_counter, kind))
    }
  }
  if r.remaining() != 0 {
    raise DecodeError("container_arena: trailing bytes")
  }
  out
}
