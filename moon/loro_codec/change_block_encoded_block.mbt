pub struct EncodedBlock {
  counter_start: UInt
  counter_len: UInt
  lamport_start: UInt
  lamport_len: UInt
  n_changes: UInt
  header: Bytes
  change_meta: Bytes
  cids: Bytes
  keys: Bytes
  positions: Bytes
  ops: Bytes
  delete_start_ids: Bytes
  values: Bytes
}

pub fn EncodedBlock::counter_start(self: EncodedBlock) -> UInt {
  self.counter_start
}

pub fn EncodedBlock::counter_len(self: EncodedBlock) -> UInt {
  self.counter_len
}

pub fn EncodedBlock::lamport_start(self: EncodedBlock) -> UInt {
  self.lamport_start
}

pub fn EncodedBlock::lamport_len(self: EncodedBlock) -> UInt {
  self.lamport_len
}

pub fn EncodedBlock::n_changes(self: EncodedBlock) -> UInt {
  self.n_changes
}

pub fn EncodedBlock::header(self: EncodedBlock) -> Bytes {
  self.header
}

pub fn EncodedBlock::change_meta(self: EncodedBlock) -> Bytes {
  self.change_meta
}

pub fn EncodedBlock::cids(self: EncodedBlock) -> Bytes {
  self.cids
}

pub fn EncodedBlock::keys(self: EncodedBlock) -> Bytes {
  self.keys
}

pub fn EncodedBlock::positions(self: EncodedBlock) -> Bytes {
  self.positions
}

pub fn EncodedBlock::ops(self: EncodedBlock) -> Bytes {
  self.ops
}

pub fn EncodedBlock::delete_start_ids(self: EncodedBlock) -> Bytes {
  self.delete_start_ids
}

pub fn EncodedBlock::values(self: EncodedBlock) -> Bytes {
  self.values
}

fn read_postcard_u32_block(r: BytesReader) -> UInt raise DecodeError {
  let v = r.read_varint_u64()
  if v > 0xFFFF_FFFFUL {
    raise DecodeError("postcard: u32 overflow")
  }
  v.to_uint()
}

fn write_postcard_u32_block(w: BytesWriter, v: UInt) -> Unit {
  w.write_varint_u64(v.to_uint64())
}

fn read_postcard_bytes(r: BytesReader) -> Bytes raise DecodeError {
  let len_u64 = r.read_varint_u64()
  if len_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("postcard: bytes too large")
  }
  let len = len_u64.to_int()
  if len < 0 || len > r.remaining() {
    raise DecodeError("postcard: invalid bytes length")
  }
  r.read_exact(len).to_bytes()
}

fn write_postcard_bytes(w: BytesWriter, b: Bytes) -> Unit {
  w.write_varint_u64(b.length().to_uint64())
  w.write_bytes(b)
}

pub fn decode_encoded_block(bytes: BytesView) -> EncodedBlock raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let counter_start = read_postcard_u32_block(r)
  let counter_len = read_postcard_u32_block(r)
  let lamport_start = read_postcard_u32_block(r)
  let lamport_len = read_postcard_u32_block(r)
  let n_changes = read_postcard_u32_block(r)
  let header = read_postcard_bytes(r)
  let change_meta = read_postcard_bytes(r)
  let cids = read_postcard_bytes(r)
  let keys = read_postcard_bytes(r)
  let positions = read_postcard_bytes(r)
  let ops = read_postcard_bytes(r)
  let delete_start_ids = read_postcard_bytes(r)
  let values = read_postcard_bytes(r)
  if r.remaining() != 0 {
    raise DecodeError("postcard: trailing bytes")
  }
  {
    counter_start,
    counter_len,
    lamport_start,
    lamport_len,
    n_changes,
    header,
    change_meta,
    cids,
    keys,
    positions,
    ops,
    delete_start_ids,
    values,
  }
}

pub fn encode_encoded_block(block: EncodedBlock) -> Bytes {
  let w = BytesWriter::new()
  write_postcard_u32_block(w, block.counter_start)
  write_postcard_u32_block(w, block.counter_len)
  write_postcard_u32_block(w, block.lamport_start)
  write_postcard_u32_block(w, block.lamport_len)
  write_postcard_u32_block(w, block.n_changes)
  write_postcard_bytes(w, block.header)
  write_postcard_bytes(w, block.change_meta)
  write_postcard_bytes(w, block.cids)
  write_postcard_bytes(w, block.keys)
  write_postcard_bytes(w, block.positions)
  write_postcard_bytes(w, block.ops)
  write_postcard_bytes(w, block.delete_start_ids)
  write_postcard_bytes(w, block.values)
  w.to_bytes()
}
