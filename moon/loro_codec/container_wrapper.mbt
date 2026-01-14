pub struct ContainerWrapper {
  kind: ContainerType
  depth: UInt64
  parent: ContainerID?
  payload: BytesView
}

pub fn ContainerWrapper::kind(self: ContainerWrapper) -> ContainerType {
  self.kind
}

pub fn ContainerWrapper::depth(self: ContainerWrapper) -> UInt64 {
  self.depth
}

pub fn ContainerWrapper::parent(self: ContainerWrapper) -> ContainerID? {
  self.parent
}

pub fn ContainerWrapper::payload_view(self: ContainerWrapper) -> BytesView {
  self.payload
}

pub fn parse_container_wrapper(bytes: BytesView) -> ContainerWrapper raise DecodeError {
  if bytes.length() < 1 {
    raise DecodeError("container_wrapper: empty bytes")
  }
  let r = BytesReader::from_view(bytes)
  let kind = container_type_from_u8(r.read_u8())
  let depth = r.read_uleb128_u64()
  let (parent, rest) = try! postcard_decode_option_container_id(bytes[bytes.length() - r.remaining():])
  { kind, depth, parent, payload: rest }
}

pub fn encode_container_wrapper(
  kind: ContainerType,
  depth: UInt64,
  parent: ContainerID?,
  payload: Bytes,
) -> Bytes {
  let w = BytesWriter::new()
  w.write_u8(container_type_to_u8(kind))
  w.write_uleb128_u64(depth)
  w.write_bytes(postcard_encode_option_container_id(parent))
  w.write_bytes(payload)
  w.to_bytes()
}
