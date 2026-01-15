///|
fn encode_keys_from_table(keys : Array[String]) -> Bytes {
  let w = BytesWriter::new()
  for key in keys {
    let b = @encoding/utf8.encode(key[:])
    w.write_uleb128_u64(b.length().to_uint64())
    w.write_bytes(b)
  }
  w.to_bytes()
}

///|
fn encode_container_arena_from_table(
  cids : Array[ContainerID],
  peers : Array[UInt64],
  peer_to_idx : @hashmap.HashMap[UInt64, UInt64],
  keys : Array[String],
  key_to_idx : @hashmap.HashMap[String, UInt64],
) -> Bytes raise EncodeError {
  let w = BytesWriter::new()
  w.write_varint_u64(cids.length().to_uint64())
  for cid in cids {
    w.write_varint_u64(4UL)
    match cid {
      ContainerID::Root(name, kind) => {
        w.write_u8(b'\x01')
        w.write_u8(container_type_to_u8(kind))
        w.write_varint_u64(0UL)
        let idx = register_key(keys, key_to_idx, name)
        if idx > 0x7FFF_FFFFUL {
          raise EncodeError("container_arena: root key idx too large")
        }
        w.write_varint_i64(idx.reinterpret_as_int64())
      }
      ContainerID::Normal(peer, counter, kind) => {
        w.write_u8(b'\x00')
        w.write_u8(container_type_to_u8(kind))
        let idx = register_peer(peers, peer_to_idx, peer)
        if idx > 0x7FFF_FFFFUL {
          raise EncodeError("container_arena: peer idx too large")
        }
        w.write_varint_u64(idx)
        w.write_varint_i64(counter.to_int64())
      }
    }
  }
  w.to_bytes()
}

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

///|
fn encode_ops_and_values(
  changes : Array[Change],
  peers : Array[UInt64],
  peer_to_idx : @hashmap.HashMap[UInt64, UInt64],
  keys : Array[String],
  key_to_idx : @hashmap.HashMap[String, UInt64],
  cids : Array[ContainerID],
  positions : Array[Bytes],
) -> (Array[EncodedOpRow], Array[EncodedDeleteStartIdRow], Bytes) raise EncodeError {
  let ops : Array[EncodedOpRow] = []
  let del_ids : Array[EncodedDeleteStartIdRow] = []
  let values_w = BytesWriter::new()
  for c in changes {
    for op in c.ops() {
      let cid_idx = register_cid(cids, op.container())
      let (prop, value, maybe_del) = match op.content() {
        OpContent::List(ListOp::Insert(pos, items)) => {
          let v = Value::LoroValue(LoroValue::List(items))
          (pos.reinterpret_as_int(), v, Option::None)
        }
        OpContent::List(ListOp::Delete(pos, del_len, start_id)) =>
          (pos, Value::DeleteSeq, Option::Some((start_id, del_len)))
        OpContent::MovableList(MovableListOp::Insert(pos, items)) => {
          let v = Value::LoroValue(LoroValue::List(items))
          (pos.reinterpret_as_int(), v, Option::None)
        }
        OpContent::MovableList(MovableListOp::Delete(pos, del_len, start_id)) =>
          (pos, Value::DeleteSeq, Option::Some((start_id, del_len)))
        OpContent::MovableList(MovableListOp::Move(from, to, elem_id)) => {
          let from_idx = register_peer(peers, peer_to_idx, elem_id.peer())
          let v = Value::ListMove({
            from: from.to_uint64(),
            from_idx,
            lamport: elem_id.lamport().to_uint64(),
          })
          (to.reinterpret_as_int(), v, Option::None)
        }
        OpContent::MovableList(MovableListOp::Set(elem_id, value)) => {
          let peer_idx = register_peer(peers, peer_to_idx, elem_id.peer())
          let v = Value::ListSet({ peer_idx, lamport: elem_id.lamport(), value })
          (0, v, Option::None)
        }
        OpContent::Map(MapOp::Insert(key, value)) => {
          let key_idx = register_key(keys, key_to_idx, key)
          if key_idx > 0x7FFF_FFFFUL {
            raise EncodeError("op: map key idx too large")
          }
          (key_idx.to_int(), Value::LoroValue(value), Option::None)
        }
        OpContent::Map(MapOp::Delete(key)) => {
          let key_idx = register_key(keys, key_to_idx, key)
          if key_idx > 0x7FFF_FFFFUL {
            raise EncodeError("op: map key idx too large")
          }
          (key_idx.to_int(), Value::DeleteOnce, Option::None)
        }
        OpContent::Text(TextOp::Insert(pos, s)) =>
          (pos.reinterpret_as_int(), Value::Str(s), Option::None)
        OpContent::Text(TextOp::Delete(pos, del_len, start_id)) =>
          (pos, Value::DeleteSeq, Option::Some((start_id, del_len)))
        OpContent::Text(TextOp::Mark(start, end, key, value, info)) => {
          let key_idx = register_key(keys, key_to_idx, key)
          if key_idx > 0x7FFF_FFFFUL {
            raise EncodeError("op: mark key idx too large")
          }
          let len_u64 = (end - start).to_uint64()
          let v = Value::MarkStart({ info, len: len_u64, key_idx, value })
          (start.reinterpret_as_int(), v, Option::None)
        }
        OpContent::Text(TextOp::MarkEnd) => (0, Value::Null, Option::None)
        OpContent::Tree(TreeOp::Create(target, parent, fi)) => {
          let subject_peer_idx = register_peer(
            peers,
            peer_to_idx,
            target.peer(),
          )
          let pos_idx = register_position(positions, fi.bytes())
          let (is_parent_null, parent_peer_idx, parent_cnt) = match parent {
            Option::None => (true, 0UL, 0)
            Option::Some(p) =>
              (false, register_peer(peers, peer_to_idx, p.peer()), p.counter())
          }
          let v = Value::RawTreeMove({
            subject_peer_idx,
            subject_cnt: target.counter(),
            position_idx: pos_idx,
            is_parent_null,
            parent_peer_idx,
            parent_cnt,
          })
          (0, v, Option::None)
        }
        OpContent::Tree(TreeOp::Move(target, parent, fi)) => {
          let subject_peer_idx = register_peer(
            peers,
            peer_to_idx,
            target.peer(),
          )
          let pos_idx = register_position(positions, fi.bytes())
          let (is_parent_null, parent_peer_idx, parent_cnt) = match parent {
            Option::None => (true, 0UL, 0)
            Option::Some(p) =>
              (false, register_peer(peers, peer_to_idx, p.peer()), p.counter())
          }
          let v = Value::RawTreeMove({
            subject_peer_idx,
            subject_cnt: target.counter(),
            position_idx: pos_idx,
            is_parent_null,
            parent_peer_idx,
            parent_cnt,
          })
          (0, v, Option::None)
        }
        OpContent::Tree(TreeOp::Delete(target)) => {
          let subject_peer_idx = register_peer(
            peers,
            peer_to_idx,
            target.peer(),
          )
          let deleted_root_peer = 0xFFFF_FFFF_FFFF_FFFFUL
          let deleted_root_cnt = 2147483647
          let parent_peer_idx = register_peer(
            peers, peer_to_idx, deleted_root_peer,
          )
          let v = Value::RawTreeMove({
            subject_peer_idx,
            subject_cnt: target.counter(),
            position_idx: 0UL,
            is_parent_null: false,
            parent_peer_idx,
            parent_cnt: deleted_root_cnt,
          })
          (0, v, Option::None)
        }
        OpContent::Future(FutureOp::Unknown(prop, raw)) =>
          (prop, raw, Option::None)
      }
      match maybe_del {
        Option::None => ()
        Option::Some((start_id, signed_len)) => {
          let peer_idx = register_peer(peers, peer_to_idx, start_id.peer())
          del_ids.push({
            peer_idx,
            counter: start_id.counter(),
            len: signed_len,
          })
        }
      }
      let (tag, content) = encode_value_content(value)
      values_w.write_bytes(content)
      ops.push({
        container_index: cid_idx,
        prop,
        value_type: tag.to_uint(),
        len: op.len(),
      })
    }
  }
  (ops, del_ids, values_w.to_bytes())
}

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
