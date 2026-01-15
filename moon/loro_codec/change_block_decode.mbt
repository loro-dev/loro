pub fn decode_change_block(bytes: BytesView) -> Array[Change] raise DecodeError {
  let doc = decode_encoded_block(bytes)

  let header = decode_changes_header(
    doc.header()[:],
    doc.n_changes(),
    doc.counter_start(),
    doc.counter_len(),
    doc.lamport_start(),
    doc.lamport_len(),
  )
  let meta = decode_changes_meta(doc.change_meta()[:], doc.n_changes())
  let keys = decode_keys(doc.keys()[:])
  let cids = decode_container_arena(doc.cids()[:], header.peers(), keys)
  let positions = decode_position_arena_v2(doc.positions()[:])
  let encoded_ops = decode_encoded_ops(doc.ops()[:])
  let delete_start_ids = decode_delete_start_ids(doc.delete_start_ids()[:])

  let n_u64 = doc.n_changes().to_uint64()
  if n_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("change_block: too many changes")
  }
  let n = n_u64.to_int()
  let changes: Array[Change] = []
  for i in 0..<n {
    changes.push(Change::new(
      ID::new(header.peer(), header.counters()[i]),
      meta.timestamps()[i],
      header.deps()[i],
      header.lamports()[i],
      meta.commit_msgs()[i],
    ))
  }

  let peer = header.peer()
  let mut counter_i64: Int64 = doc.counter_start().to_int64()
  let mut change_idx = 0
  let mut del_idx = 0
  let mut values_rest = doc.values()[:]

  for row in encoded_ops {
    if row.container_index > 0x7FFF_FFFFU {
      raise DecodeError("change_block: container_index too large")
    }
    let cid_i = row.container_index.reinterpret_as_int()
    if cid_i < 0 || cid_i >= cids.length() {
      raise DecodeError("change_block: invalid container_index")
    }
    let cid = cids[cid_i]
    let tag = (row.value_type & 0xFF).to_byte()
    let (value, rest) = decode_value_content(tag, values_rest)
    values_rest = rest

    if counter_i64 < -2147483648L || counter_i64 > 2147483647L {
      raise DecodeError("change_block: counter overflow")
    }
    let counter = counter_i64.to_int()
    let op_id = ID::new(peer, counter)
    let (content, next_del_idx) = decode_op_content(
      cid,
      row.prop,
      row.len,
      value,
      header.peers(),
      keys,
      positions,
      delete_start_ids,
      del_idx,
      op_id,
    )
    del_idx = next_del_idx

    if change_idx < 0 || change_idx >= changes.length() {
      raise DecodeError("change_block: change index overflow")
    }
    changes[change_idx].ops().push(Op::new(cid, counter, row.len, content))

    counter_i64 = counter_i64 + row.len.to_int64()
    if change_idx + 1 < header.counters().length() {
      let next_boundary = header.counters()[change_idx + 1].to_int64()
      if counter_i64 > next_boundary {
        raise DecodeError("change_block: op len overflow change boundary")
      }
      if counter_i64 == next_boundary {
        change_idx = change_idx + 1
      }
    }
  }

  if values_rest.length() != 0 {
    raise DecodeError("change_block: trailing value bytes")
  }
  if del_idx != delete_start_ids.length() {
    raise DecodeError("change_block: unused delete_start_ids")
  }

  changes
}

pub struct DecodedChangeBlock {
  peers: Array[UInt64]
  keys: Array[String]
  cids: Array[ContainerID]
  positions: Array[Bytes]
  changes: Array[Change]
}

pub fn DecodedChangeBlock::peers(self: DecodedChangeBlock) -> Array[UInt64] {
  self.peers
}

pub fn DecodedChangeBlock::keys(self: DecodedChangeBlock) -> Array[String] {
  self.keys
}

pub fn DecodedChangeBlock::cids(self: DecodedChangeBlock) -> Array[ContainerID] {
  self.cids
}

pub fn DecodedChangeBlock::positions(self: DecodedChangeBlock) -> Array[Bytes] {
  self.positions
}

pub fn DecodedChangeBlock::changes(self: DecodedChangeBlock) -> Array[Change] {
  self.changes
}

pub fn decode_change_block_full(bytes: BytesView) -> DecodedChangeBlock raise DecodeError {
  let doc = decode_encoded_block(bytes)
  let header = decode_changes_header(
    doc.header()[:],
    doc.n_changes(),
    doc.counter_start(),
    doc.counter_len(),
    doc.lamport_start(),
    doc.lamport_len(),
  )
  let keys = decode_keys(doc.keys()[:])
  let cids = decode_container_arena(doc.cids()[:], header.peers(), keys)
  let positions = decode_position_arena_v2(doc.positions()[:])
  let changes = decode_change_block(bytes)
  { peers: header.peers(), keys, cids, positions, changes }
}

fn change_atom_len_u64(c: Change) -> UInt64 {
  let mut sum: UInt64 = 0
  for op in c.ops() {
    sum = sum + op.len().to_uint64()
  }
  sum
}

fn init_u64_index(xs: Array[UInt64]) -> @hashmap.HashMap[UInt64, UInt64] {
  let m = @hashmap.new(capacity=xs.length())
  for i in 0..<xs.length() {
    m.set(xs[i], i.to_uint64())
  }
  m
}

fn init_string_index(xs: Array[String]) -> @hashmap.HashMap[String, UInt64] {
  let m = @hashmap.new(capacity=xs.length())
  for i in 0..<xs.length() {
    m.set(xs[i], i.to_uint64())
  }
  m
}

fn register_peer(
  peers: Array[UInt64],
  peer_to_idx: @hashmap.HashMap[UInt64, UInt64],
  peer: UInt64,
) -> UInt64 {
  match peer_to_idx.get(peer) {
    Some(idx) => idx
    None => {
      let idx = peers.length().to_uint64()
      peers.push(peer)
      peer_to_idx.set(peer, idx)
      idx
    }
  }
}

fn register_key(
  keys: Array[String],
  key_to_idx: @hashmap.HashMap[String, UInt64],
  key: String,
) -> UInt64 {
  match key_to_idx.get(key) {
    Some(idx) => idx
    None => {
      let idx = keys.length().to_uint64()
      keys.push(key)
      key_to_idx.set(key, idx)
      idx
    }
  }
}

fn register_cid(cids: Array[ContainerID], cid: ContainerID) -> UInt {
  for i in 0..<cids.length() {
    if cids[i] == cid {
      return i.reinterpret_as_uint()
    }
  }
  let idx = cids.length().reinterpret_as_uint()
  cids.push(cid)
  idx
}

fn register_position(positions: Array[Bytes], position: Bytes) -> UInt64 {
  for i in 0..<positions.length() {
    if positions[i] == position {
      return i.to_uint64()
    }
  }
  let idx = positions.length().to_uint64()
  positions.push(position)
  idx
}
