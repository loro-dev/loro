pub struct ID {
  peer: UInt64
  counter: Int
} derive(Eq, Show)

pub fn ID::new(peer: UInt64, counter: Int) -> ID {
  { peer, counter }
}

pub fn ID::peer(self: ID) -> UInt64 {
  self.peer
}

pub fn ID::counter(self: ID) -> Int {
  self.counter
}

pub fn ID::to_change_block_key(self: ID) -> Bytes {
  let w = BytesWriter::new()
  w.write_u64_be(self.peer)
  w.write_u32_be(self.counter.reinterpret_as_uint())
  w.to_bytes()
}

pub fn id_from_change_block_key(bytes: BytesView) -> ID raise DecodeError {
  if bytes.length() != 12 {
    raise DecodeError("id: invalid change block key length")
  }
  let r = BytesReader::from_view(bytes)
  let peer = r.read_u64_be()
  let counter = r.read_u32_be().reinterpret_as_int()
  if r.remaining() != 0 {
    raise DecodeError("id: trailing bytes")
  }
  { peer, counter }
}
