///|
pub fn postcard_decode_option_container_id(
  bytes : BytesView,
) -> (ContainerID?, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let opt_tag = r.read_varint_u64()
  match opt_tag {
    0UL => return (None, bytes[bytes.length() - r.remaining():])
    1UL => ()
    _ => raise DecodeError("postcard: invalid Option tag")
  }
  let cid_tag = r.read_varint_u64()
  match cid_tag {
    0UL => {
      // Root: (name: String, container_type: ContainerType as historical u8)
      let name_len_u64 = r.read_varint_u64()
      if name_len_u64 > 0x7FFF_FFFFUL {
        raise DecodeError("postcard: name too long")
      }
      let name_len = name_len_u64.to_int()
      if name_len < 0 || name_len > r.remaining() {
        raise DecodeError("postcard: invalid name length")
      }
      let name_bytes = r.read_exact(name_len)
      let name = @encoding/utf8.decode(name_bytes) catch {
        @encoding/utf8.Malformed(_) =>
          raise DecodeError("postcard: invalid utf8 name")
      }
      let kind = container_type_from_historical_u8(r.read_u8())
      return (
        Some(ContainerID::Root(name, kind)),
        bytes[bytes.length() - r.remaining():],
      )
    }
    1UL => {
      // Normal: (peer: u64 varint, counter: i32 zigzag varint, container_type as historical u8)
      let peer = r.read_varint_u64()
      let counter = r.read_varint_i64().to_int()
      let kind = container_type_from_historical_u8(r.read_u8())
      return (
        Some(ContainerID::Normal(peer, counter, kind)),
        bytes[bytes.length() - r.remaining():],
      )
    }
    _ => raise DecodeError("postcard: invalid ContainerID tag")
  }
}

///|
pub fn postcard_encode_option_container_id(cid : ContainerID?) -> Bytes {
  let w = BytesWriter::new()
  match cid {
    None => {
      w.write_varint_u64(0)
      return w.to_bytes()
    }
    Some(cid) => {
      w.write_varint_u64(1)
      match cid {
        ContainerID::Root(name, kind) => {
          w.write_varint_u64(0)
          let name_bytes = @encoding/utf8.encode(name[:])
          w.write_varint_u64(name_bytes.length().to_uint64())
          w.write_bytes(name_bytes)
          w.write_u8(container_type_to_historical_u8(kind))
        }
        ContainerID::Normal(peer, counter, kind) => {
          w.write_varint_u64(1)
          w.write_varint_u64(peer)
          w.write_varint_i64(counter.to_int64())
          w.write_u8(container_type_to_historical_u8(kind))
        }
      }
      w.to_bytes()
    }
  }
}
