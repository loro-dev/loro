///|
pub fn postcard_take_container_id(
  bytes : BytesView,
) -> (ContainerID, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let cid_tag = r.read_varint_u64()
  match cid_tag {
    0UL => {
      let name = read_postcard_utf8(r)
      let kind = container_type_from_historical_u8(r.read_u8())
      (ContainerID::root(name, kind), r.remaining_view())
    }
    1UL => {
      let peer = r.read_varint_u64()
      let counter_i64 = r.read_varint_i64()
      if counter_i64 < -2147483648L || counter_i64 > 2147483647L {
        raise DecodeError("postcard: container counter overflow")
      }
      let kind = container_type_from_historical_u8(r.read_u8())
      (
        ContainerID::normal(peer, counter_i64.to_int(), kind),
        r.remaining_view(),
      )
    }
    _ => raise DecodeError("postcard: invalid ContainerID tag")
  }
}

///|
pub fn postcard_encode_container_id(cid : ContainerID) -> Bytes {
  let w = BytesWriter::new()
  match cid {
    ContainerID::Root(name, kind) => {
      w.write_varint_u64(0)
      write_postcard_utf8(w, name)
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
