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
