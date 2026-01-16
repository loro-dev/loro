///|
fn op_content_json_schema(
  container : ContainerID,
  content : OpContent,
  keys : Array[String],
  op_id : ID,
  peers : Array[UInt64],
  peer_to_idx : @hashmap.HashMap[UInt64, UInt64],
) -> Json raise DecodeError {
  match content {
    OpContent::Text(TextOp::Insert(pos, text)) =>
      { "type": "insert", "pos": pos.reinterpret_as_int(), "text": text }
    OpContent::Text(TextOp::Delete(pos, len, start_id)) =>
      {
        "type": "delete",
        "pos": pos,
        "len": jsonschema_number_i64(len),
        "start_id": id_string_with_peer_index(start_id, peers, peer_to_idx),
      }
    OpContent::Text(TextOp::Mark(start, end, style_key, style_value, info)) =>
      {
        "type": "mark",
        "start": start.reinterpret_as_int(),
        "end": end.reinterpret_as_int(),
        "style_key": style_key,
        "style_value": loro_value_json_schema(
          style_value, keys, op_id, peers, peer_to_idx, 0,
        ),
        "info": info.to_uint().reinterpret_as_int(),
      }
    OpContent::Text(TextOp::MarkEnd) => { "type": "mark_end" }
    OpContent::List(ListOp::Insert(pos, values)) => {
      let out : Array[Json] = []
      for i in 0..<values.length() {
        let child_id = id_inc(op_id, i)
        out.push(
          loro_value_json_schema(
            values[i],
            keys,
            child_id,
            peers,
            peer_to_idx,
            0,
          ),
        )
      }
      {
        "type": "insert",
        "pos": pos.reinterpret_as_int(),
        "value": Json::array(out),
      }
    }
    OpContent::List(ListOp::Delete(pos, len, start_id)) =>
      {
        "type": "delete",
        "pos": pos,
        "len": jsonschema_number_i64(len),
        "start_id": id_string_with_peer_index(start_id, peers, peer_to_idx),
      }
    OpContent::MovableList(MovableListOp::Insert(pos, values)) => {
      let out : Array[Json] = []
      for i in 0..<values.length() {
        let child_id = id_inc(op_id, i)
        out.push(
          loro_value_json_schema(
            values[i],
            keys,
            child_id,
            peers,
            peer_to_idx,
            0,
          ),
        )
      }
      {
        "type": "insert",
        "pos": pos.reinterpret_as_int(),
        "value": Json::array(out),
      }
    }
    OpContent::MovableList(MovableListOp::Delete(pos, len, start_id)) =>
      {
        "type": "delete",
        "pos": pos,
        "len": jsonschema_number_i64(len),
        "start_id": id_string_with_peer_index(start_id, peers, peer_to_idx),
      }
    OpContent::MovableList(MovableListOp::Move(from, to, elem_id)) =>
      {
        "type": "move",
        "from": from.reinterpret_as_int(),
        "to": to.reinterpret_as_int(),
        "elem_id": idlp_string_with_peer_index(elem_id, peers, peer_to_idx),
      }
    OpContent::MovableList(MovableListOp::Set(elem_id, value)) =>
      {
        "type": "set",
        "elem_id": idlp_string_with_peer_index(elem_id, peers, peer_to_idx),
        "value": loro_value_json_schema(
          value, keys, op_id, peers, peer_to_idx, 0,
        ),
      }
    OpContent::Map(MapOp::Insert(key, value)) =>
      {
        "type": "insert",
        "key": key,
        "value": loro_value_json_schema(
          value, keys, op_id, peers, peer_to_idx, 0,
        ),
      }
    OpContent::Map(MapOp::Delete(key)) => { "type": "delete", "key": key }
    OpContent::Tree(TreeOp::Create(target, parent, fi)) =>
      {
        "type": "create",
        "target": id_string_with_peer_index(target, peers, peer_to_idx),
        "parent": match parent {
          None => Json::null()
          Some(p) =>
            Json::string(id_string_with_peer_index(p, peers, peer_to_idx))
        },
        "fractional_index": jsonschema_bytes_hex_upper(fi.bytes()),
      }
    OpContent::Tree(TreeOp::Move(target, parent, fi)) =>
      {
        "type": "move",
        "target": id_string_with_peer_index(target, peers, peer_to_idx),
        "parent": match parent {
          None => Json::null()
          Some(p) =>
            Json::string(id_string_with_peer_index(p, peers, peer_to_idx))
        },
        "fractional_index": jsonschema_bytes_hex_upper(fi.bytes()),
      }
    OpContent::Tree(TreeOp::Delete(target)) =>
      {
        "type": "delete",
        "target": id_string_with_peer_index(target, peers, peer_to_idx),
      }
    OpContent::Future(FutureOp::Unknown(prop, raw)) => {
      let kind = match container {
        ContainerID::Root(_, k) => k
        ContainerID::Normal(_, _, k) => k
      }
      match kind {
        ContainerType::Counter =>
          match raw {
            // Match Rust JsonSchema `FutureOp::Counter(OwnedValue::F64(..))`.
            Value::F64(x) => {
              let s = x.to_string()
              let repr = if s.contains(".") || s.contains("e") || s.contains("E") {
                s
              } else {
                s + ".0"
              }
              {
                "type": "counter",
                "prop": prop,
                "value_type": "f64",
                "value": Json::number(x, repr=repr),
              }
            }
            Value::I64(x) =>
              {
                "type": "counter",
                "prop": prop,
                "value_type": "f64",
                "value": Json::number(x.to_double(), repr=x.to_string() + ".0"),
              }
            _ =>
              raise DecodeError(
                "jsonschema: Counter op must have f64 raw value",
              )
          }
        _ =>
          // Best-effort: keep unknown ops opaque. This is only used when container type is Unknown.
          {
            "type": "unknown",
            "prop": prop,
            "value_type": "unknown",
            "value": Json::null(),
          }
      }
    }
  }
}

///|
fn op_json_schema(
  op : Op,
  keys : Array[String],
  change_peer : UInt64,
  peers : Array[UInt64],
  peer_to_idx : @hashmap.HashMap[UInt64, UInt64],
) -> Json raise DecodeError {
  let op_id = ID::new(change_peer, op.counter())
  {
    "container": container_id_string_with_peer_index(
      op.container(),
      peers,
      peer_to_idx,
    ),
    "content": op_content_json_schema(
      op.container(),
      op.content(),
      keys,
      op_id,
      peers,
      peer_to_idx,
    ),
    "counter": jsonschema_number_int(op.counter()),
  }
}
