///|
fn take_peer_table(
  bytes : BytesView,
) -> (Array[UInt64], BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let n_u64 = r.read_uleb128_u64()
  if n_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("state: too many peers")
  }
  let n = n_u64.to_int()
  let peers : Array[UInt64] = []
  for _i in 0..<n {
    peers.push(r.read_u64_le())
  }
  (peers, r.remaining_view())
}

///|
fn encode_peer_table(peers : Array[UInt64]) -> Bytes {
  let w = BytesWriter::new()
  w.write_uleb128_u64(peers.length().to_uint64())
  for p in peers {
    w.write_u64_le(p)
  }
  w.to_bytes()
}
