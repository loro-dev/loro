///|
pub struct ChangesHeader {
  peer : UInt64
  peers : Array[UInt64]
  counters : Array[Int]
  lengths : Array[Int]
  lamports : Array[UInt]
  deps : Array[Array[ID]]
}

///|
pub fn ChangesHeader::peer(self : ChangesHeader) -> UInt64 {
  self.peer
}

///|
pub fn ChangesHeader::peers(self : ChangesHeader) -> Array[UInt64] {
  self.peers
}

///|
pub fn ChangesHeader::counters(self : ChangesHeader) -> Array[Int] {
  self.counters
}

///|
pub fn ChangesHeader::lengths(self : ChangesHeader) -> Array[Int] {
  self.lengths
}

///|
pub fn ChangesHeader::lamports(self : ChangesHeader) -> Array[UInt] {
  self.lamports
}

///|
pub fn ChangesHeader::deps(self : ChangesHeader) -> Array[Array[ID]] {
  self.deps
}

///|
pub fn decode_changes_header(
  bytes : BytesView,
  n_changes : UInt,
  counter_start : UInt,
  counter_len : UInt,
  lamport_start : UInt,
  lamport_len : UInt,
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
  let peers : Array[UInt64] = []
  for _i in 0..<peer_num {
    peers.push(r.read_u64_le())
  }
  let lengths : Array[Int] = []
  let mut sum : Int = 0
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
  let deps_len : Array[Int] = []
  let mut other_dep_num : Int = 0
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
  let (dep_peers_u64, rest3) = any_rle_take_n_finalize_usize(
    rest2, other_dep_num,
  )
  let dep_peers : Array[Int] = []
  for x in dep_peers_u64 {
    if x > 0x7FFF_FFFFUL {
      raise DecodeError("change_header: dep_peer_idx too large")
    }
    dep_peers.push(x.to_int())
  }
  let (dep_counters_i64, rest4) = delta_of_delta_take_n_finalize_i64(
    rest3, other_dep_num,
  )
  let dep_counters : Array[Int] = []
  for x in dep_counters_i64 {
    if x < 0L || x > 2147483647L {
      raise DecodeError("change_header: dep counter overflow")
    }
    dep_counters.push(x.to_int())
  }
  let deps : Array[Array[ID]] = []
  let mut this_counter = first_counter
  let mut dep_idx = 0
  for i in 0..<n {
    let ids : Array[ID] = []
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
  let counters : Array[Int] = []
  let mut cur = first_counter
  for i in 0..<lengths.length() {
    counters.push(cur)
    cur = cur + lengths[i]
  }
  counters.push(first_counter + counter_len_i)
  let (lamports_i64, rest5) = delta_of_delta_take_n_finalize_i64(rest4, n - 1)
  let lamports : Array[UInt] = []
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
