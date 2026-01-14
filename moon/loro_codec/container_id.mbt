pub enum ContainerType {
  Text
  Map
  List
  MovableList
  Tree
  Counter
  Unknown(UInt)
} derive(Eq, Show)

// NOTE: MoonBit makes enum constructors read-only across packages. Expose explicit
// constructor functions for blackbox tests and future callers.
pub fn ContainerType::text() -> ContainerType {
  ContainerType::Text
}

pub fn ContainerType::map() -> ContainerType {
  ContainerType::Map
}

pub fn ContainerType::list() -> ContainerType {
  ContainerType::List
}

pub fn ContainerType::movable_list() -> ContainerType {
  ContainerType::MovableList
}

pub fn ContainerType::tree() -> ContainerType {
  ContainerType::Tree
}

pub fn ContainerType::counter() -> ContainerType {
  ContainerType::Counter
}

pub fn ContainerType::unknown(kind: UInt) -> ContainerType {
  ContainerType::Unknown(kind)
}

pub fn container_type_to_u8(t: ContainerType) -> Byte {
  match t {
    ContainerType::Map => b'\x00'
    ContainerType::List => b'\x01'
    ContainerType::Text => b'\x02'
    ContainerType::Tree => b'\x03'
    ContainerType::MovableList => b'\x04'
    ContainerType::Counter => b'\x05'
    ContainerType::Unknown(k) => (k & 0xFF).to_byte()
  }
}

pub fn container_type_from_u8(b: Byte) -> ContainerType {
  match b.to_uint() {
    0 => ContainerType::Map
    1 => ContainerType::List
    2 => ContainerType::Text
    3 => ContainerType::Tree
    4 => ContainerType::MovableList
    5 => ContainerType::Counter
    x => ContainerType::Unknown(x)
  }
}

// Used by serde (postcard) for historical reasons.
pub fn container_type_to_historical_u8(t: ContainerType) -> Byte {
  match t {
    ContainerType::Text => b'\x00'
    ContainerType::Map => b'\x01'
    ContainerType::List => b'\x02'
    ContainerType::MovableList => b'\x03'
    ContainerType::Tree => b'\x04'
    ContainerType::Counter => b'\x05'
    ContainerType::Unknown(k) => (k & 0xFF).to_byte()
  }
}

pub fn container_type_from_historical_u8(b: Byte) -> ContainerType {
  match b.to_uint() {
    0 => ContainerType::Text
    1 => ContainerType::Map
    2 => ContainerType::List
    3 => ContainerType::MovableList
    4 => ContainerType::Tree
    5 => ContainerType::Counter
    x => ContainerType::Unknown(x)
  }
}

pub enum ContainerID {
  Root(String, ContainerType)
  Normal(UInt64, Int, ContainerType)
} derive(Eq, Show)

pub fn ContainerID::root(name: String, kind: ContainerType) -> ContainerID {
  ContainerID::Root(name, kind)
}

pub fn ContainerID::normal(peer: UInt64, counter: Int, kind: ContainerType) -> ContainerID {
  ContainerID::Normal(peer, counter, kind)
}

pub fn ContainerID::to_bytes(self: ContainerID) -> Bytes {
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

pub fn container_id_from_bytes(bytes: BytesView) -> ContainerID raise DecodeError {
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
    let name = @encoding/utf8.decode(name_bytes) catch { @encoding/utf8.Malformed(_) =>
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

// --- postcard (serde) encoding for Option<ContainerID> ---

pub fn postcard_decode_option_container_id(
  bytes: BytesView,
) -> (ContainerID?, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let opt_tag = r.read_varint_u64()
  match opt_tag {
    0UL => {
      return (None, bytes[bytes.length() - r.remaining():])
    }
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
      let name = @encoding/utf8.decode(name_bytes) catch { @encoding/utf8.Malformed(_) =>
        raise DecodeError("postcard: invalid utf8 name")
      }
      let kind = container_type_from_historical_u8(r.read_u8())
      return (Some(ContainerID::Root(name, kind)), bytes[bytes.length() - r.remaining():])
    }
    1UL => {
      // Normal: (peer: u64 varint, counter: i32 zigzag varint, container_type as historical u8)
      let peer = r.read_varint_u64()
      let counter = r.read_varint_i64().to_int()
      let kind = container_type_from_historical_u8(r.read_u8())
      return (Some(ContainerID::Normal(peer, counter, kind)), bytes[bytes.length() - r.remaining():])
    }
    _ => raise DecodeError("postcard: invalid ContainerID tag")
  }
}

pub fn postcard_encode_option_container_id(cid: ContainerID?) -> Bytes {
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
