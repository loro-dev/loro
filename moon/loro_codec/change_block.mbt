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

pub struct ChangesHeader {
  peer: UInt64
  peers: Array[UInt64]
  counters: Array[Int]
  lengths: Array[Int]
  lamports: Array[UInt]
  deps: Array[Array[ID]]
}

pub fn ChangesHeader::peer(self: ChangesHeader) -> UInt64 {
  self.peer
}

pub fn ChangesHeader::peers(self: ChangesHeader) -> Array[UInt64] {
  self.peers
}

pub fn ChangesHeader::counters(self: ChangesHeader) -> Array[Int] {
  self.counters
}

pub fn ChangesHeader::lengths(self: ChangesHeader) -> Array[Int] {
  self.lengths
}

pub fn ChangesHeader::lamports(self: ChangesHeader) -> Array[UInt] {
  self.lamports
}

pub fn ChangesHeader::deps(self: ChangesHeader) -> Array[Array[ID]] {
  self.deps
}

pub fn decode_changes_header(
  bytes: BytesView,
  n_changes: UInt,
  counter_start: UInt,
  counter_len: UInt,
  lamport_start: UInt,
  lamport_len: UInt,
) -> ChangesHeader raise DecodeError {
  let n_u64 = n_changes.to_uint64()
  if n_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("change_header: too many changes")
  }
  let n = n_u64.to_int()
  if n <= 0 {
    raise DecodeError("change_header: empty block")
  }

  let first_counter = counter_start.reinterpret_as_int()
  let counter_len_i = counter_len.reinterpret_as_int()
  if counter_len_i < 0 {
    raise DecodeError("change_header: invalid counter_len")
  }

  let r = BytesReader::from_view(bytes)

  let peer_num_u64 = r.read_uleb128_u64()
  if peer_num_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("change_header: too many peers")
  }
  let peer_num = peer_num_u64.to_int()
  if peer_num <= 0 {
    raise DecodeError("change_header: empty peer table")
  }

  let peers: Array[UInt64] = []
  for _i in 0..<peer_num {
    peers.push(r.read_u64_le())
  }

  let lengths: Array[Int] = []
  let mut sum: Int = 0
  for _i in 0..<(n - 1) {
    let len_u64 = r.read_uleb128_u64()
    if len_u64 > 0x7FFF_FFFFUL {
      raise DecodeError("change_header: atom_len too large")
    }
    let len_i = len_u64.to_int()
    if len_i < 0 {
      raise DecodeError("change_header: invalid atom_len")
    }
    sum = sum + len_i
    lengths.push(len_i)
  }

  let last_len = counter_len_i - sum
  if last_len < 0 {
    raise DecodeError("change_header: invalid atom_len sum")
  }
  lengths.push(last_len)

  let (dep_self, rest1) = bool_rle_take_n_finalize(r.remaining_view(), n)
  let (dep_lens_u64, rest2) = any_rle_take_n_finalize_usize(rest1, n)
  let deps_len: Array[Int] = []
  let mut other_dep_num: Int = 0
  for x in dep_lens_u64 {
    if x > 0x7FFF_FFFFUL {
      raise DecodeError("change_header: dep_len too large")
    }
    let xi = x.to_int()
    if xi < 0 {
      raise DecodeError("change_header: invalid dep_len")
    }
    other_dep_num = other_dep_num + xi
    deps_len.push(xi)
  }

  let (dep_peers_u64, rest3) = any_rle_take_n_finalize_usize(rest2, other_dep_num)
  let dep_peers: Array[Int] = []
  for x in dep_peers_u64 {
    if x > 0x7FFF_FFFFUL {
      raise DecodeError("change_header: dep_peer_idx too large")
    }
    dep_peers.push(x.to_int())
  }

  let (dep_counters_i64, rest4) = delta_of_delta_take_n_finalize_i64(rest3, other_dep_num)
  let dep_counters: Array[Int] = []
  for x in dep_counters_i64 {
    if x < 0L || x > 2147483647L {
      raise DecodeError("change_header: dep counter overflow")
    }
    dep_counters.push(x.to_int())
  }

  let deps: Array[Array[ID]] = []
  let mut this_counter = first_counter
  let mut dep_idx = 0
  for i in 0..<n {
    let ids: Array[ID] = []
    if dep_self[i] {
      ids.push(ID::new(peers[0], this_counter - 1))
    }
    let len = deps_len[i]
    for _j in 0..<len {
      if dep_idx >= dep_peers.length() || dep_idx >= dep_counters.length() {
        raise DecodeError("change_header: deps underflow")
      }
      let peer_idx = dep_peers[dep_idx]
      if peer_idx < 0 || peer_idx >= peers.length() {
        raise DecodeError("change_header: invalid dep peer idx")
      }
      ids.push(ID::new(peers[peer_idx], dep_counters[dep_idx]))
      dep_idx = dep_idx + 1
    }
    deps.push(ids)
    this_counter = this_counter + lengths[i]
  }
  if dep_idx != dep_peers.length() || dep_idx != dep_counters.length() {
    raise DecodeError("change_header: deps trailing")
  }

  let counters: Array[Int] = []
  let mut cur = first_counter
  for i in 0..<lengths.length() {
    counters.push(cur)
    cur = cur + lengths[i]
  }
  counters.push(first_counter + counter_len_i)

  let (lamports_i64, rest5) = delta_of_delta_take_n_finalize_i64(rest4, n - 1)
  let lamports: Array[UInt] = []
  for x in lamports_i64 {
    if x < 0L || x > 0xFFFF_FFFFL {
      raise DecodeError("change_header: lamport overflow")
    }
    lamports.push(x.reinterpret_as_uint64().to_uint())
  }
  let last_len_u64 = lengths[n - 1].to_uint64()
  let last_lamport_u64 = lamport_start.to_uint64() + lamport_len.to_uint64()
  if last_lamport_u64 < last_len_u64 {
    raise DecodeError("change_header: invalid lamport range")
  }
  let last_lamport = (last_lamport_u64 - last_len_u64).to_uint()
  lamports.push(last_lamport)

  if rest5.length() != 0 {
    raise DecodeError("change_header: trailing bytes")
  }

  { peer: peers[0], peers, counters, lengths, lamports, deps }
}

pub struct ChangesMeta {
  timestamps: Array[Int64]
  commit_msgs: Array[String?]
}

pub fn ChangesMeta::timestamps(self: ChangesMeta) -> Array[Int64] {
  self.timestamps
}

pub fn ChangesMeta::commit_msgs(self: ChangesMeta) -> Array[String?] {
  self.commit_msgs
}

pub fn decode_changes_meta(bytes: BytesView, n_changes: UInt) -> ChangesMeta raise DecodeError {
  let n_u64 = n_changes.to_uint64()
  if n_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("change_meta: too many changes")
  }
  let n = n_u64.to_int()
  if n <= 0 {
    raise DecodeError("change_meta: empty block")
  }

  let (timestamps, rest1) = delta_of_delta_take_n_finalize_i64(bytes, n)
  let (lens_u32, rest2) = any_rle_take_n_finalize_u32(rest1, n)
  let mut total: Int = 0
  let lens: Array[Int] = []
  for x in lens_u32 {
    let xi = x.reinterpret_as_int()
    if xi < 0 {
      raise DecodeError("change_meta: invalid msg len")
    }
    total = total + xi
    lens.push(xi)
  }
  if total < 0 || total > rest2.length() {
    raise DecodeError("change_meta: invalid msg bytes length")
  }

  let msgs: Array[String?] = []
  let mut offset = 0
  for len in lens {
    if len == 0 {
      msgs.push(None)
      continue
    }
    let end = offset + len
    if end < 0 || end > rest2.length() {
      raise DecodeError("change_meta: msg bytes overflow")
    }
    let s = @encoding/utf8.decode(rest2[offset:end]) catch { @encoding/utf8.Malformed(_) =>
      raise DecodeError("change_meta: invalid utf8 msg")
    }
    msgs.push(Some(s))
    offset = end
  }
  if offset != total {
    raise DecodeError("change_meta: msg bytes trailing")
  }
  { timestamps, commit_msgs: msgs }
}

pub fn decode_keys(bytes: BytesView) -> Array[String] raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let out: Array[String] = []
  while r.remaining() > 0 {
    let len_u64 = r.read_uleb128_u64()
    if len_u64 > 0x7FFF_FFFFUL {
      raise DecodeError("keys: key too long")
    }
    let len = len_u64.to_int()
    if len < 0 || len > r.remaining() {
      raise DecodeError("keys: invalid key length")
    }
    let key_bytes = r.read_exact(len)
    let key = @encoding/utf8.decode(key_bytes) catch { @encoding/utf8.Malformed(_) =>
      raise DecodeError("keys: invalid utf8")
    }
    out.push(key)
  }
  out
}

pub fn decode_container_arena(
  bytes: BytesView,
  peers: Array[UInt64],
  keys: Array[String],
) -> Array[ContainerID] raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let n_u64 = r.read_varint_u64()
  if n_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("container_arena: too many containers")
  }
  let n = n_u64.to_int()
  if n < 0 {
    raise DecodeError("container_arena: invalid len")
  }

  let out: Array[ContainerID] = []
  for _i in 0..<n {
    let is_root = match r.read_u8() {
      b'\x00' => false
      b'\x01' => true
      _ => raise DecodeError("container_arena: invalid bool")
    }
    let kind = container_type_from_u8(r.read_u8())
    let peer_idx_u64 = r.read_varint_u64()
    if peer_idx_u64 > 0x7FFF_FFFFUL {
      raise DecodeError("container_arena: peer_idx too large")
    }
    let peer_idx = peer_idx_u64.to_int()
    let key_idx_or_counter_i64 = r.read_varint_i64()
    if key_idx_or_counter_i64 < -2147483648L || key_idx_or_counter_i64 > 2147483647L {
      raise DecodeError("container_arena: i32 overflow")
    }
    let key_idx_or_counter = key_idx_or_counter_i64.to_int()
    if is_root {
      if key_idx_or_counter < 0 || key_idx_or_counter >= keys.length() {
        raise DecodeError("container_arena: invalid root key idx")
      }
      out.push(ContainerID::root(keys[key_idx_or_counter], kind))
    } else {
      if peer_idx < 0 || peer_idx >= peers.length() {
        raise DecodeError("container_arena: invalid peer idx")
      }
      out.push(ContainerID::normal(peers[peer_idx], key_idx_or_counter, kind))
    }
  }
  if r.remaining() != 0 {
    raise DecodeError("container_arena: trailing bytes")
  }
  out
}
