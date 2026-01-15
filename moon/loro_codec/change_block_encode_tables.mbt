///|
fn encode_keys_from_table(keys : Array[String]) -> Bytes {
  let w = BytesWriter::new()
  for key in keys {
    let b = @encoding/utf8.encode(key[:])
    w.write_uleb128_u64(b.length().to_uint64())
    w.write_bytes(b)
  }
  w.to_bytes()
}

///|
fn encode_container_arena_from_table(
  cids : Array[ContainerID],
  peers : Array[UInt64],
  peer_to_idx : @hashmap.HashMap[UInt64, UInt64],
  keys : Array[String],
  key_to_idx : @hashmap.HashMap[String, UInt64],
) -> Bytes raise EncodeError {
  let w = BytesWriter::new()
  w.write_varint_u64(cids.length().to_uint64())
  for cid in cids {
    w.write_varint_u64(4UL)
    match cid {
      ContainerID::Root(name, kind) => {
        w.write_u8(b'\x01')
        w.write_u8(container_type_to_u8(kind))
        w.write_varint_u64(0UL)
        let idx = register_key(keys, key_to_idx, name)
        if idx > 0x7FFF_FFFFUL {
          raise EncodeError("container_arena: root key idx too large")
        }
        w.write_varint_i64(idx.reinterpret_as_int64())
      }
      ContainerID::Normal(peer, counter, kind) => {
        w.write_u8(b'\x00')
        w.write_u8(container_type_to_u8(kind))
        let idx = register_peer(peers, peer_to_idx, peer)
        if idx > 0x7FFF_FFFFUL {
          raise EncodeError("container_arena: peer idx too large")
        }
        w.write_varint_u64(idx)
        w.write_varint_i64(counter.to_int64())
      }
    }
  }
  w.to_bytes()
}
