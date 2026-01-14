pub struct ParsedDocument {
  mode: UInt
  body: BytesView
}

pub fn ParsedDocument::mode(self: ParsedDocument) -> UInt {
  self.mode
}

pub fn ParsedDocument::body(self: ParsedDocument) -> Bytes {
  self.body.to_bytes()
}

pub fn ParsedDocument::body_view(self: ParsedDocument) -> BytesView {
  self.body
}

pub struct FastSnapshotBody {
  oplog_bytes: BytesView
  state_bytes: BytesView
  shallow_root_state_bytes: BytesView
}

pub fn FastSnapshotBody::oplog_bytes_view(self: FastSnapshotBody) -> BytesView {
  self.oplog_bytes
}

pub fn FastSnapshotBody::state_bytes_view(self: FastSnapshotBody) -> BytesView {
  self.state_bytes
}

pub fn FastSnapshotBody::shallow_root_state_bytes_view(self: FastSnapshotBody) -> BytesView {
  self.shallow_root_state_bytes
}

const MAGIC_BYTES: Bytes = b"loro"
const MIN_HEADER_SIZE: Int = 22

pub fn parse_document(bytes: Bytes, check_checksum: Bool) -> ParsedDocument raise DecodeError {
  if bytes.length() < MIN_HEADER_SIZE {
    raise DecodeError("invalid document: too short")
  }
  if bytes[0:4] != MAGIC_BYTES[:] {
    raise DecodeError("invalid document: magic mismatch")
  }

  // checksum field is bytes[4..20], but only last 4 bytes are used for FastSnapshot/FastUpdates.
  let stored_checksum = BytesReader::from_view(bytes[16:20]).read_u32_le()

  let mode = BytesReader::from_view(bytes[20:22]).read_u16_be()
  let body = bytes[22:]

  if check_checksum {
    // IMPORTANT: checksum covers bytes[20..] (mode + body), not just body.
    let checksum_body = bytes[20:]
    let expected = xxhash32(checksum_body, LORO_XXH32_SEED)
    if expected != stored_checksum {
      raise DecodeError("invalid document: checksum mismatch")
    }
  }

  { mode, body }
}

pub fn parse_fast_snapshot_body(body: BytesView) -> FastSnapshotBody raise DecodeError {
  let r = BytesReader::from_view(body)

  let oplog_len = r.read_u32_le().reinterpret_as_int()
  if oplog_len < 0 || oplog_len > r.remaining() {
    raise DecodeError("fastsnapshot: invalid oplog length")
  }
  let oplog_bytes = r.read_exact(oplog_len)

  let state_len = r.read_u32_le().reinterpret_as_int()
  if state_len < 0 || state_len > r.remaining() {
    raise DecodeError("fastsnapshot: invalid state length")
  }
  let state_bytes = r.read_exact(state_len)

  let shallow_len = r.read_u32_le().reinterpret_as_int()
  if shallow_len < 0 || shallow_len > r.remaining() {
    raise DecodeError("fastsnapshot: invalid shallow_root_state length")
  }
  let shallow_root_state_bytes = r.read_exact(shallow_len)

  if r.remaining() != 0 {
    raise DecodeError("fastsnapshot: trailing bytes")
  }

  { oplog_bytes, state_bytes, shallow_root_state_bytes }
}

pub fn parse_fast_updates_body(body: BytesView) -> Array[BytesView] raise DecodeError {
  let r = BytesReader::from_view(body)
  let blocks: Array[BytesView] = []
  while r.remaining() > 0 {
    let len_u64 = r.read_uleb128_u64()
    if len_u64 > 0x7FFF_FFFFUL {
      raise DecodeError("fastupdates: block too large")
    }
    let len = len_u64.to_int()
    if len < 0 || len > r.remaining() {
      raise DecodeError("fastupdates: invalid block length")
    }
    blocks.push(r.read_exact(len))
  }
  blocks
}

pub fn encode_fast_snapshot_body(oplog: Bytes, state: Bytes, shallow_root_state: Bytes) -> Bytes {
  let w = BytesWriter::new()
  w.write_u32_le(oplog.length().reinterpret_as_uint())
  w.write_bytes(oplog)
  w.write_u32_le(state.length().reinterpret_as_uint())
  w.write_bytes(state)
  w.write_u32_le(shallow_root_state.length().reinterpret_as_uint())
  w.write_bytes(shallow_root_state)
  w.to_bytes()
}

pub fn encode_fast_updates_body(blocks: Array[Bytes]) -> Bytes {
  let w = BytesWriter::new()
  for b in blocks {
    w.write_uleb128_u64(b.length().to_uint64())
    w.write_bytes(b)
  }
  w.to_bytes()
}

pub fn encode_document(mode: UInt, body: Bytes) -> Bytes raise DecodeError {
  // checksum covers bytes[20..] = mode(u16 BE) + body
  let checksum_input = BytesWriter::new()
  checksum_input.write_u16_be(mode)
  checksum_input.write_bytes(body)
  let checksum = xxhash32(checksum_input.to_bytes()[:], LORO_XXH32_SEED)

  let w = BytesWriter::new()
  w.write_bytes(MAGIC_BYTES)
  for _i in 0..<12 {
    w.write_u8(0x00)
  }
  w.write_u32_le(checksum)
  w.write_u16_be(mode)
  w.write_bytes(body)
  w.to_bytes()
}

pub fn transcode_document(bytes: Bytes, validate: Bool) -> Bytes raise DecodeError {
  let doc = parse_document(bytes, true)
  match doc.mode() {
    3 => {
      let parsed = parse_fast_snapshot_body(doc.body_view())
      // v1: keep bytes as-is, but reconstruct a canonical blob with recomputed checksum.
      let body = encode_fast_snapshot_body(
        parsed.oplog_bytes_view().to_bytes(),
        parsed.state_bytes_view().to_bytes(),
        parsed.shallow_root_state_bytes_view().to_bytes(),
      )
      if validate {
        // TODO: validate SSTable/state decoding in later milestones.
      }
      encode_document(3, body)
    }
    4 => {
      let blocks = parse_fast_updates_body(doc.body_view())
      if validate {
        for b in blocks {
          let _ = decode_change_block(b)
        }
      }
      let out_blocks: Array[Bytes] = []
      for b in blocks {
        out_blocks.push(b.to_bytes())
      }
      let body = encode_fast_updates_body(out_blocks)
      encode_document(4, body)
    }
    _ => raise DecodeError("unsupported document mode")
  }
}
