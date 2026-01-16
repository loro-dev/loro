// --- VersionVector / Frontiers ---

///|
pub type VersionVector = Array[(UInt64, Int)] // (peer, counter)

///|
pub fn postcard_take_version_vector(
  bytes : BytesView,
) -> (VersionVector, BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let len_u64 = r.read_varint_u64()
  if len_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("postcard: vv too large")
  }
  let len = len_u64.to_int()
  let out : VersionVector = []
  for _i in 0..<len {
    let peer = r.read_varint_u64()
    let counter_i64 = r.read_varint_i64()
    if counter_i64 < -2147483648L || counter_i64 > 2147483647L {
      raise DecodeError("postcard: vv counter overflow")
    }
    out.push((peer, counter_i64.to_int()))
  }
  (out, r.remaining_view())
}

///|
pub fn postcard_encode_version_vector(vv : VersionVector) -> Bytes {
  let w = BytesWriter::new()
  w.write_varint_u64(vv.length().to_uint64())
  for pair in vv {
    let (peer, counter) = pair
    w.write_varint_u64(peer)
    w.write_varint_i64(counter.to_int64())
  }
  w.to_bytes()
}

///|
pub fn postcard_take_frontiers(
  bytes : BytesView,
) -> (Array[ID], BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let len_u64 = r.read_varint_u64()
  if len_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("postcard: frontiers too large")
  }
  let len = len_u64.to_int()
  let out : Array[ID] = []
  for _i in 0..<len {
    let peer = r.read_varint_u64()
    let counter_i64 = r.read_varint_i64()
    if counter_i64 < -2147483648L || counter_i64 > 2147483647L {
      raise DecodeError("postcard: frontier counter overflow")
    }
    out.push(ID::new(peer, counter_i64.to_int()))
  }
  (out, r.remaining_view())
}

///|
pub fn postcard_encode_frontiers(ids : Array[ID]) -> Bytes {
  let sorted : Array[ID] = []
  for id in ids {
    sorted.push(id)
  }
  // Canonical encoding: sort by (peer, counter).
  let counter_bias = BigInt::from_int64(-2147483648L)
  sorted.sort_by_key(id => {
    let peer_key = BigInt::from_uint64(id.peer()) << 32
    let counter_key = BigInt::from_int(id.counter()) - counter_bias
    peer_key | counter_key
  })
  let w = BytesWriter::new()
  w.write_varint_u64(sorted.length().to_uint64())
  for id in sorted {
    w.write_varint_u64(id.peer())
    w.write_varint_i64(id.counter().to_int64())
  }
  w.to_bytes()
}
