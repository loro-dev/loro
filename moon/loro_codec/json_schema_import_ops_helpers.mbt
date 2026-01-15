///|
fn jsonschema_import_parse_values_array(
  values_json : Array[Json],
  op_id : ID,
  keys : Array[String],
  key_to_idx : @hashmap.HashMap[String, UInt64],
  peers : Array[UInt64]?,
) -> Array[LoroValue] raise DecodeError {
  let values : Array[LoroValue] = []
  for i in 0..<values_json.length() {
    values.push(
      jsonschema_import_loro_value(
        values_json[i],
        id_inc(op_id, i),
        keys,
        key_to_idx,
        peers,
      ),
    )
  }
  values
}

///|
fn jsonschema_import_parse_tree_parent(
  obj : Map[String, Json],
  peers : Array[UInt64]?,
) -> ID? raise DecodeError {
  match obj.get("parent") {
    None => None
    Some(vp) =>
      match vp {
        Json::Null => None
        _ =>
          Some(
            jsonschema_import_parse_id(
              jsonschema_import_expect_string(vp, "tree.parent"),
              peers,
              "tree_id",
            ),
          )
      }
  }
}
