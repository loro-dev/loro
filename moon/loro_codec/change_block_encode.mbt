///|
pub fn encode_change_block(
  block : DecodedChangeBlock,
) -> Bytes raise EncodeError {
  let changes = block.changes()
  if changes.length() == 0 {
    raise EncodeError("change_block: empty changes")
  }

  // Mutable working tables (can append new items if needed).
  let peers : Array[UInt64] = []
  for p in block.peers() {
    peers.push(p)
  }
  let peer_to_idx = init_u64_index(peers)
  let keys : Array[String] = []
  for k in block.keys() {
    keys.push(k)
  }
  let key_to_idx = init_string_index(keys)
  let cids : Array[ContainerID] = []
  for cid in block.cids() {
    cids.push(cid)
  }
  let positions : Array[Bytes] = []
  for p in block.positions() {
    positions.push(p)
  }

  // Encode ops/values first; this may append to peers/keys/cids/positions.
  let (encoded_ops, delete_start_ids, values) = encode_ops_and_values(
    changes, peers, peer_to_idx, keys, key_to_idx, cids, positions,
  )

  // Encode container arena next; this may append to peers/keys.
  let cids_bytes = encode_container_arena_from_table(
    cids, peers, peer_to_idx, keys, key_to_idx,
  )
  let keys_bytes = encode_keys_from_table(keys)
  let positions_bytes = encode_position_arena_v2(positions)
  let ops_bytes = encode_encoded_ops(encoded_ops)
  let delete_start_ids_bytes = encode_delete_start_ids(delete_start_ids)

  // Encode header/meta last; this may append to peers (via deps).
  let header = encode_changes_header_from_changes(changes, peers, peer_to_idx)
  let change_meta = encode_changes_meta_from_changes(changes)

  // Derive block-level ranges.
  let first = changes[0]
  let last = changes[changes.length() - 1]
  let counter_start = first.id().counter()
  let mut counter_len_u64 : UInt64 = 0
  for c in changes {
    counter_len_u64 = counter_len_u64 + change_atom_len_u64(c)
  }
  if counter_start < 0 || counter_start > 2147483647 {
    raise EncodeError("change_block: counter_start overflow")
  }
  if counter_len_u64 > 0xFFFF_FFFFUL {
    raise EncodeError("change_block: counter_len overflow")
  }
  let lamport_start = first.lamport()
  let last_len = change_atom_len_u64(last)
  let lamport_end_u64 = last.lamport().to_uint64() + last_len
  if lamport_end_u64 < lamport_start.to_uint64() {
    raise EncodeError("change_block: invalid lamport range")
  }
  let lamport_len_u64 = lamport_end_u64 - lamport_start.to_uint64()
  if lamport_len_u64 > 0xFFFF_FFFFUL {
    raise EncodeError("change_block: lamport_len overflow")
  }
  let out = {
    counter_start: counter_start.reinterpret_as_uint(),
    counter_len: counter_len_u64.to_uint(),
    lamport_start,
    lamport_len: lamport_len_u64.to_uint(),
    n_changes: changes.length().reinterpret_as_uint(),
    header,
    change_meta,
    cids: cids_bytes,
    keys: keys_bytes,
    positions: positions_bytes,
    ops: ops_bytes,
    delete_start_ids: delete_start_ids_bytes,
    values,
  }
  encode_encoded_block(out)
}
