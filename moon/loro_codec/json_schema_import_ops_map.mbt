///|
fn jsonschema_import_parse_map_op_content(
  obj : Map[String, Json],
  t : String,
  op_id : ID,
  keys : Array[String],
  key_to_idx : @hashmap.HashMap[String, UInt64],
  peers : Array[UInt64]?,
) -> OpContent raise DecodeError {
  match t {
    "insert" => {
      let key = jsonschema_import_req_string(obj, "key", "map.insert.key")
      let value = jsonschema_import_loro_value(
        jsonschema_import_req(obj, "value", "map.insert.value"),
        op_id,
        keys,
        key_to_idx,
        peers,
      )
      OpContent::Map(MapOp::Insert(key, value))
    }
    "delete" => {
      let key = jsonschema_import_req_string(obj, "key", "map.delete.key")
      OpContent::Map(MapOp::Delete(key))
    }
    _ => raise DecodeError("jsonschema_import: unknown map op type: " + t)
  }
}
