///|
fn jsonschema_import_op_len(content : OpContent) -> UInt {
  match content {
    OpContent::Map(_) => 1
    OpContent::Tree(_) => 1
    OpContent::Future(_) => 1
    OpContent::List(ListOp::Insert(_pos, values)) =>
      values.length().reinterpret_as_uint()
    OpContent::List(ListOp::Delete(_pos, len, _start_id)) =>
      len.abs().reinterpret_as_uint64().to_uint()
    OpContent::MovableList(MovableListOp::Insert(_pos, values)) =>
      values.length().reinterpret_as_uint()
    OpContent::MovableList(MovableListOp::Delete(_pos, len, _start_id)) =>
      len.abs().reinterpret_as_uint64().to_uint()
    OpContent::MovableList(MovableListOp::Move(_, _, _)) => 1
    OpContent::MovableList(MovableListOp::Set(_, _)) => 1
    OpContent::Text(TextOp::Insert(_pos, text)) =>
      count_utf8_codepoints(text).reinterpret_as_uint()
    OpContent::Text(TextOp::Delete(_pos, len, _start_id)) =>
      len.abs().reinterpret_as_uint64().to_uint()
    OpContent::Text(TextOp::Mark(_, _, _, _, _)) => 1
    OpContent::Text(TextOp::MarkEnd) => 1
  }
}

///|
fn jsonschema_import_parse_op_content(
  container : ContainerID,
  v : Json,
  op_id : ID,
  keys : Array[String],
  key_to_idx : @hashmap.HashMap[String, UInt64],
  peers : Array[UInt64]?,
) -> OpContent raise DecodeError {
  let obj = jsonschema_import_expect_object(v, "op.content")
  let t = match obj.get("type") {
    Some(vt) => jsonschema_import_expect_string(vt, "op.content.type")
    None => raise DecodeError("jsonschema_import: missing op.content.type")
  }
  let kind = match container {
    ContainerID::Root(_, k) => k
    ContainerID::Normal(_, _, k) => k
  }
  match kind {
    ContainerType::Text =>
      jsonschema_import_parse_text_op_content(
        obj, t, op_id, keys, key_to_idx, peers,
      )
    ContainerType::List =>
      jsonschema_import_parse_list_op_content(
        obj, t, op_id, keys, key_to_idx, peers,
      )
    ContainerType::MovableList =>
      jsonschema_import_parse_movable_list_op_content(
        obj, t, op_id, keys, key_to_idx, peers,
      )
    ContainerType::Map =>
      jsonschema_import_parse_map_op_content(
        obj, t, op_id, keys, key_to_idx, peers,
      )
    ContainerType::Tree =>
      jsonschema_import_parse_tree_op_content(obj, t, peers)
    ContainerType::Counter =>
      raise DecodeError("jsonschema_import: Counter ops are not supported")
    ContainerType::Unknown(_) =>
      raise DecodeError(
        "jsonschema_import: Unknown container type is not supported",
      )
  }
}
