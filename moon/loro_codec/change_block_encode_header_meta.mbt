///|
fn encode_changes_header_from_changes(
  changes : Array[Change],
  peers : Array[UInt64],
  peer_to_idx : @hashmap.HashMap[UInt64, UInt64],
) -> Bytes raise EncodeError {
  if changes.length() == 0 {
    raise EncodeError("change_block: empty changes")
  }
  let peer0 = changes[0].id().peer()
  if peers.length() == 0 {
    peers.push(peer0)
    peer_to_idx.set(peer0, 0UL)
  } else if peers[0] != peer0 {
    raise EncodeError("change_block: peers[0] must be block peer")
  }
  let n = changes.length()
  let dep_on_self : Array[Bool] = []
  let dep_lens : Array[UInt64] = []
  let dep_peer_idxs : Array[UInt64] = []
  let dep_counters : Array[Int64] = []
  for c in changes {
    let mut on_self = false
    for dep in c.deps() {
      if dep.peer() == peer0 {
        on_self = true
      } else {
        dep_peer_idxs.push(register_peer(peers, peer_to_idx, dep.peer()))
        dep_counters.push(dep.counter().to_int64())
      }
    }
    dep_on_self.push(on_self)
    let dep_len = if on_self {
      (c.deps().length() - 1).to_uint64()
    } else {
      c.deps().length().to_uint64()
    }
    dep_lens.push(dep_len)
  }
  let w = BytesWriter::new()
  w.write_uleb128_u64(peers.length().to_uint64())
  for p in peers {
    w.write_u64_le(p)
  }
  for i in 0..<(n - 1) {
    let atom_len = change_atom_len_u64(changes[i])
    w.write_uleb128_u64(atom_len)
  }
  w.write_bytes(encode_bool_rle(dep_on_self))
  w.write_bytes(encode_any_rle_usize(dep_lens))
  w.write_bytes(encode_any_rle_usize(dep_peer_idxs))
  w.write_bytes(encode_delta_of_delta_i64(dep_counters))
  let lamports : Array[Int64] = []
  for i in 0..<(n - 1) {
    lamports.push(changes[i].lamport().to_int64())
  }
  w.write_bytes(encode_delta_of_delta_i64(lamports))
  w.to_bytes()
}

///|
fn encode_changes_meta_from_changes(changes : Array[Change]) -> Bytes {
  let timestamps : Array[Int64] = []
  let lens_u32 : Array[UInt] = []
  let msgs_w = BytesWriter::new()
  for c in changes {
    timestamps.push(c.timestamp())
    match c.msg() {
      None => lens_u32.push(0)
      Some(s) => {
        let b = @encoding/utf8.encode(s[:])
        lens_u32.push(b.length().reinterpret_as_uint())
        msgs_w.write_bytes(b)
      }
    }
  }
  let w = BytesWriter::new()
  w.write_bytes(encode_delta_of_delta_i64(timestamps))
  w.write_bytes(encode_any_rle_u32(lens_u32))
  w.write_bytes(msgs_w.to_bytes())
  w.to_bytes()
}
