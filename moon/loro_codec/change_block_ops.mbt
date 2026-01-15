pub struct EncodedOpRow {
  container_index: UInt
  prop: Int
  value_type: UInt
  len: UInt
} derive(Eq, Show)

pub fn decode_encoded_ops(bytes: BytesView) -> Array[EncodedOpRow] raise DecodeError {
  if bytes.length() == 0 {
    return []
  }
  let cols = decode_columnar_vec_maybe_wrapped(bytes)
  if cols.length() != 4 {
    raise DecodeError("encoded_ops: invalid column count")
  }
  let container_indices = decode_delta_rle_u32(cols[0])
  let props = decode_delta_rle_i32(cols[1])
  let value_types = decode_rle_u8(cols[2])
  let lens = decode_rle_u32(cols[3])

  let n = container_indices.length()
  if props.length() != n || value_types.length() != n || lens.length() != n {
    raise DecodeError("encoded_ops: column length mismatch")
  }

  let out: Array[EncodedOpRow] = []
  for i in 0..<n {
    out.push({
      container_index: container_indices[i],
      prop: props[i],
      value_type: value_types[i],
      len: lens[i],
    })
  }
  out
}

pub fn encode_encoded_ops(ops: Array[EncodedOpRow]) -> Bytes {
  if ops.length() == 0 {
    return encode_columnar_vec_wrapped([b"", b"", b"", b""])
  }
  let container_indices: Array[UInt] = []
  let props: Array[Int] = []
  let value_types: Array[UInt] = []
  let lens: Array[UInt] = []
  for op in ops {
    container_indices.push(op.container_index)
    props.push(op.prop)
    value_types.push(op.value_type)
    lens.push(op.len)
  }
  let col0 = encode_delta_rle_u32(container_indices)
  let col1 = encode_delta_rle_i32(props)
  let col2 = encode_rle_u8(value_types)
  let col3 = encode_rle_u32(lens)
  encode_columnar_vec_wrapped([col0, col1, col2, col3])
}

pub struct EncodedDeleteStartIdRow {
  peer_idx: UInt64
  counter: Int
  len: Int64
} derive(Eq, Show)

pub fn decode_delete_start_ids(bytes: BytesView) -> Array[EncodedDeleteStartIdRow] raise DecodeError {
  if bytes.length() == 0 {
    return []
  }
  let cols = decode_columnar_vec_maybe_wrapped(bytes)
  if cols.length() != 3 {
    raise DecodeError("delete_start_ids: invalid column count")
  }
  let peer_idxs = decode_delta_rle_usize(cols[0])
  let counters = decode_delta_rle_i32(cols[1])
  let lens = decode_delta_rle_isize(cols[2])
  let n = peer_idxs.length()
  if counters.length() != n || lens.length() != n {
    raise DecodeError("delete_start_ids: column length mismatch")
  }
  let out: Array[EncodedDeleteStartIdRow] = []
  for i in 0..<n {
    out.push({ peer_idx: peer_idxs[i], counter: counters[i], len: lens[i] })
  }
  out
}

pub fn encode_delete_start_ids(ids: Array[EncodedDeleteStartIdRow]) -> Bytes {
  if ids.length() == 0 {
    return b""
  }
  let peer_idxs: Array[UInt64] = []
  let counters: Array[Int] = []
  let lens: Array[Int64] = []
  for id in ids {
    peer_idxs.push(id.peer_idx)
    counters.push(id.counter)
    lens.push(id.len)
  }
  let col0 = encode_delta_rle_usize(peer_idxs)
  let col1 = encode_delta_rle_i32(counters)
  let col2 = encode_delta_rle_isize(lens)
  encode_columnar_vec_wrapped([col0, col1, col2])
}

fn is_deleted_tree_root(peer: UInt64, counter: Int) -> Bool {
  peer == 0xFFFF_FFFF_FFFF_FFFFUL && counter == 2147483647
}

fn decode_op_content(
  cid: ContainerID,
  prop: Int,
  _len: UInt,
  value: Value,
  peers: Array[UInt64],
  keys: Array[String],
  positions: Array[Bytes],
  delete_start_ids: Array[EncodedDeleteStartIdRow],
  del_idx: Int,
  op_id: ID,
) -> (OpContent, Int) raise DecodeError {
  let kind = match cid {
    ContainerID::Root(_, k) => k
    ContainerID::Normal(_, _, k) => k
  }

  match kind {
    ContainerType::Map => {
      if prop < 0 || prop >= keys.length() {
        raise DecodeError("op: invalid map key idx")
      }
      let key = keys[prop]
      match value {
        Value::DeleteOnce => (OpContent::Map(MapOp::Delete(key)), del_idx)
        Value::LoroValue(v) => (OpContent::Map(MapOp::Insert(key, v)), del_idx)
        _ => raise DecodeError("op: invalid map value kind")
      }
    }
    ContainerType::Text => {
      match value {
        Value::Str(s) => {
          if prop < 0 {
            raise DecodeError("op: invalid text insert pos")
          }
          (OpContent::Text(TextOp::Insert(prop.reinterpret_as_uint(), s)), del_idx)
        }
        Value::DeleteSeq => {
          if del_idx < 0 || del_idx >= delete_start_ids.length() {
            raise DecodeError("op: delete_start_ids underflow")
          }
          let del = delete_start_ids[del_idx]
          if del.peer_idx > 0x7FFF_FFFFUL {
            raise DecodeError("op: delete_start peer_idx too large")
          }
          let peer_i = del.peer_idx.to_int()
          if peer_i < 0 || peer_i >= peers.length() {
            raise DecodeError("op: delete_start invalid peer_idx")
          }
          let start_id = ID::new(peers[peer_i], del.counter)
          (OpContent::Text(TextOp::Delete(prop, del.len, start_id)), del_idx + 1)
        }
        Value::MarkStart(m) => {
          if prop < 0 {
            raise DecodeError("op: invalid mark start")
          }
          if m.len > 0xFFFF_FFFFUL {
            raise DecodeError("op: mark len overflow")
          }
          if m.key_idx > 0x7FFF_FFFFUL {
            raise DecodeError("op: mark key_idx too large")
          }
          let key_i = m.key_idx.to_int()
          if key_i < 0 || key_i >= keys.length() {
            raise DecodeError("op: invalid mark key idx")
          }
          let start_u = prop.reinterpret_as_uint()
          let end_u = start_u + m.len.to_uint()
          (OpContent::Text(TextOp::Mark(start_u, end_u, keys[key_i], m.value, m.info)), del_idx)
        }
        Value::Null => (OpContent::Text(TextOp::MarkEnd), del_idx)
        _ => raise DecodeError("op: invalid text value kind")
      }
    }
    ContainerType::List => {
      match value {
        Value::LoroValue(LoroValue::List(items)) => {
          if prop < 0 {
            raise DecodeError("op: invalid list insert pos")
          }
          (OpContent::List(ListOp::Insert(prop.reinterpret_as_uint(), items)), del_idx)
        }
        Value::DeleteSeq => {
          if del_idx < 0 || del_idx >= delete_start_ids.length() {
            raise DecodeError("op: delete_start_ids underflow")
          }
          let del = delete_start_ids[del_idx]
          if del.peer_idx > 0x7FFF_FFFFUL {
            raise DecodeError("op: delete_start peer_idx too large")
          }
          let peer_i = del.peer_idx.to_int()
          if peer_i < 0 || peer_i >= peers.length() {
            raise DecodeError("op: delete_start invalid peer_idx")
          }
          let start_id = ID::new(peers[peer_i], del.counter)
          (OpContent::List(ListOp::Delete(prop, del.len, start_id)), del_idx + 1)
        }
        _ => raise DecodeError("op: invalid list value kind")
      }
    }
    ContainerType::MovableList => {
      match value {
        Value::LoroValue(LoroValue::List(items)) => {
          if prop < 0 {
            raise DecodeError("op: invalid movable_list insert pos")
          }
          (OpContent::MovableList(MovableListOp::Insert(prop.reinterpret_as_uint(), items)), del_idx)
        }
        Value::DeleteSeq => {
          if del_idx < 0 || del_idx >= delete_start_ids.length() {
            raise DecodeError("op: delete_start_ids underflow")
          }
          let del = delete_start_ids[del_idx]
          if del.peer_idx > 0x7FFF_FFFFUL {
            raise DecodeError("op: delete_start peer_idx too large")
          }
          let peer_i = del.peer_idx.to_int()
          if peer_i < 0 || peer_i >= peers.length() {
            raise DecodeError("op: delete_start invalid peer_idx")
          }
          let start_id = ID::new(peers[peer_i], del.counter)
          (OpContent::MovableList(MovableListOp::Delete(prop, del.len, start_id)), del_idx + 1)
        }
        Value::ListMove(m) => {
          if m.from > 0xFFFF_FFFFUL {
            raise DecodeError("op: movable_list move from overflow")
          }
          if m.lamport > 0xFFFF_FFFFUL {
            raise DecodeError("op: movable_list move lamport overflow")
          }
          if m.from_idx > 0x7FFF_FFFFUL {
            raise DecodeError("op: movable_list move peer_idx too large")
          }
          let peer_i = m.from_idx.to_int()
          if peer_i < 0 || peer_i >= peers.length() {
            raise DecodeError("op: movable_list move invalid peer_idx")
          }
          if prop < 0 {
            raise DecodeError("op: movable_list move invalid to")
          }
          let elem = IdLp::new(peers[peer_i], m.lamport.to_uint())
          (OpContent::MovableList(MovableListOp::Move(m.from.to_uint(), prop.reinterpret_as_uint(), elem)), del_idx)
        }
        Value::ListSet(s) => {
          if s.peer_idx > 0x7FFF_FFFFUL {
            raise DecodeError("op: movable_list set peer_idx too large")
          }
          let peer_i = s.peer_idx.to_int()
          if peer_i < 0 || peer_i >= peers.length() {
            raise DecodeError("op: movable_list set invalid peer_idx")
          }
          let elem = IdLp::new(peers[peer_i], s.lamport)
          (OpContent::MovableList(MovableListOp::Set(elem, s.value)), del_idx)
        }
        _ => raise DecodeError("op: invalid movable_list value kind")
      }
    }
    ContainerType::Tree => {
      match value {
        Value::RawTreeMove(t) => {
          if t.subject_peer_idx > 0x7FFF_FFFFUL || t.parent_peer_idx > 0x7FFF_FFFFUL {
            raise DecodeError("op: tree peer_idx too large")
          }
          let subject_peer_i = t.subject_peer_idx.to_int()
          if subject_peer_i < 0 || subject_peer_i >= peers.length() {
            raise DecodeError("op: tree invalid subject peer_idx")
          }
          let subject = ID::new(peers[subject_peer_i], t.subject_cnt)
          let parent: ID? =
            if t.is_parent_null {
              None
            } else {
              let parent_peer_i = t.parent_peer_idx.to_int()
              if parent_peer_i < 0 || parent_peer_i >= peers.length() {
                raise DecodeError("op: tree invalid parent peer_idx")
              }
              Some(ID::new(peers[parent_peer_i], t.parent_cnt))
            }

          match parent {
            Some(p) => {
              if is_deleted_tree_root(p.peer(), p.counter()) {
                return (OpContent::Tree(TreeOp::Delete(subject)), del_idx)
              }
            }
            None => ()
          }

          if t.position_idx > 0x7FFF_FFFFUL {
            raise DecodeError("op: tree position_idx too large")
          }
          let pos_i = t.position_idx.to_int()
          if pos_i < 0 || pos_i >= positions.length() {
            raise DecodeError("op: tree invalid position_idx")
          }
          let fi = FractionalIndex::new(positions[pos_i])
          let is_create = subject.peer() == op_id.peer() && subject.counter() == op_id.counter()
          if is_create {
            (OpContent::Tree(TreeOp::Create(subject, parent, fi)), del_idx)
          } else {
            (OpContent::Tree(TreeOp::Move(subject, parent, fi)), del_idx)
          }
        }
        _ => raise DecodeError("op: invalid tree value kind")
      }
    }
    _ => {
      // Counter/Unknown container types: keep as opaque future op.
      (OpContent::Future(FutureOp::Unknown(prop, value)), del_idx)
    }
  }
}
