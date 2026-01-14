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
