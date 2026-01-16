///|
fn jsonschema_import_parse_movable_list_op_content(
  obj : Map[String, Json],
  t : String,
  op_id : ID,
  keys : Array[String],
  key_to_idx : @hashmap.HashMap[String, UInt64],
  peers : Array[UInt64]?,
) -> OpContent raise DecodeError {
  match t {
    "insert" => {
      let pos = jsonschema_import_req_u32(obj, "pos", "mlist.insert.pos")
      let values_json = jsonschema_import_req_array(
        obj, "value", "mlist.insert.value",
      )
      let values = jsonschema_import_parse_values_array(
        values_json, op_id, keys, key_to_idx, peers,
      )
      OpContent::MovableList(MovableListOp::Insert(pos, values))
    }
    "delete" => {
      let pos = jsonschema_import_req_int(obj, "pos", "mlist.delete.pos")
      let len_i64 = jsonschema_import_req_i64(obj, "len", "mlist.delete.len")
      let start_id = jsonschema_import_parse_id(
        jsonschema_import_req_string(obj, "start_id", "mlist.delete.start_id"),
        peers,
        "start_id",
      )
      OpContent::MovableList(MovableListOp::Delete(pos, len_i64, start_id))
    }
    "move" => {
      let from = jsonschema_import_req_u32(obj, "from", "mlist.move.from")
      let to = jsonschema_import_req_u32(obj, "to", "mlist.move.to")
      let elem_id = jsonschema_import_parse_idlp(
        jsonschema_import_req_string(obj, "elem_id", "mlist.move.elem_id"),
        peers,
        "elem_id",
      )
      OpContent::MovableList(MovableListOp::Move(from, to, elem_id))
    }
    "set" => {
      let elem_id = jsonschema_import_parse_idlp(
        jsonschema_import_req_string(obj, "elem_id", "mlist.set.elem_id"),
        peers,
        "elem_id",
      )
      let value = jsonschema_import_loro_value(
        jsonschema_import_req(obj, "value", "mlist.set.value"),
        op_id,
        keys,
        key_to_idx,
        peers,
      )
      OpContent::MovableList(MovableListOp::Set(elem_id, value))
    }
    _ =>
      raise DecodeError("jsonschema_import: unknown movable_list op type: " + t)
  }
}
