///|
fn jsonschema_import_parse_list_op_content(
  obj : Map[String, Json],
  t : String,
  op_id : ID,
  keys : Array[String],
  key_to_idx : @hashmap.HashMap[String, UInt64],
  peers : Array[UInt64]?,
) -> OpContent raise DecodeError {
  match t {
    "insert" => {
      let pos = jsonschema_import_req_u32(obj, "pos", "list.insert.pos")
      let values_json = jsonschema_import_req_array(
        obj, "value", "list.insert.value",
      )
      let values = jsonschema_import_parse_values_array(
        values_json, op_id, keys, key_to_idx, peers,
      )
      OpContent::List(ListOp::Insert(pos, values))
    }
    "delete" => {
      let pos = jsonschema_import_req_int(obj, "pos", "list.delete.pos")
      let len_i64 = jsonschema_import_req_i64(obj, "len", "list.delete.len")
      let start_id = jsonschema_import_parse_id(
        jsonschema_import_req_string(obj, "start_id", "list.delete.start_id"),
        peers,
        "start_id",
      )
      OpContent::List(ListOp::Delete(pos, len_i64, start_id))
    }
    _ => raise DecodeError("jsonschema_import: unknown list op type: " + t)
  }
}
