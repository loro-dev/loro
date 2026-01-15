///|
pub enum ContainerID {
  Root(String, ContainerType)
  Normal(UInt64, Int, ContainerType)
} derive(Eq, Show)

///|
pub fn ContainerID::root(name : String, kind : ContainerType) -> ContainerID {
  ContainerID::Root(name, kind)
}

///|
pub fn ContainerID::normal(
  peer : UInt64,
  counter : Int,
  kind : ContainerType,
) -> ContainerID {
  ContainerID::Normal(peer, counter, kind)
}

///|
pub fn ContainerID::to_bytes(self : ContainerID) -> Bytes {
  let w = BytesWriter::new()
  match self {
    ContainerID::Root(name, kind) => {
      let first = container_type_to_u8(kind).to_uint() | 0x80
      w.write_u8((first & 0xFF).to_byte())
      let name_bytes = @encoding/utf8.encode(name[:])
      w.write_uleb128_u64(name_bytes.length().to_uint64())
      w.write_bytes(name_bytes)
    }
    ContainerID::Normal(peer, counter, kind) => {
      w.write_u8(container_type_to_u8(kind))
      w.write_u64_le(peer)
      w.write_u32_le(counter.reinterpret_as_uint())
    }
  }
  w.to_bytes()
}

///|
pub fn container_id_from_bytes(
  bytes : BytesView,
) -> ContainerID raise DecodeError {
  if bytes.length() < 1 {
    raise DecodeError("container_id: empty bytes")
  }
  let r = BytesReader::from_view(bytes)
  let first = r.read_u8().to_uint()
  let kind = container_type_from_u8((first & 0x7F).to_byte())
  let is_root = (first & 0x80) != 0
  if is_root {
    let name_len_u64 = r.read_uleb128_u64()
    if name_len_u64 > 0x7FFF_FFFFUL {
      raise DecodeError("container_id: name too long")
    }
    let name_len = name_len_u64.to_int()
    if name_len < 0 || name_len > r.remaining() {
      raise DecodeError("container_id: invalid name length")
    }
    let name_bytes = r.read_exact(name_len)
    let name = @encoding/utf8.decode(name_bytes) catch {
      @encoding/utf8.Malformed(_) =>
        raise DecodeError("container_id: invalid utf8 name")
    }
    if r.remaining() != 0 {
      raise DecodeError("container_id: trailing bytes")
    }
    return ContainerID::Root(name, kind)
  }
  if r.remaining() != 12 {
    raise DecodeError("container_id: invalid normal length")
  }
  let peer = r.read_u64_le()
  let counter = r.read_u32_le().reinterpret_as_int()
  if r.remaining() != 0 {
    raise DecodeError("container_id: trailing bytes")
  }
  ContainerID::Normal(peer, counter, kind)
}
