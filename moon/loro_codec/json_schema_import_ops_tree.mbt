///|
fn jsonschema_import_parse_tree_op_content(
  obj : Map[String, Json],
  t : String,
  peers : Array[UInt64]?,
) -> OpContent raise DecodeError {
  match t {
    "create" => {
      let target = jsonschema_import_parse_id(
        jsonschema_import_req_string(obj, "target", "tree.create.target"),
        peers,
        "tree_id",
      )
      let parent = jsonschema_import_parse_tree_parent(obj, peers)
      let fi_hex = jsonschema_import_req_string(
        obj, "fractional_index", "tree.fractional_index",
      )
      let fi = FractionalIndex::new(jsonschema_import_hex_to_bytes(fi_hex))
      OpContent::Tree(TreeOp::Create(target, parent, fi))
    }
    "move" => {
      let target = jsonschema_import_parse_id(
        jsonschema_import_req_string(obj, "target", "tree.move.target"),
        peers,
        "tree_id",
      )
      let parent = jsonschema_import_parse_tree_parent(obj, peers)
      let fi_hex = jsonschema_import_req_string(
        obj, "fractional_index", "tree.fractional_index",
      )
      let fi = FractionalIndex::new(jsonschema_import_hex_to_bytes(fi_hex))
      OpContent::Tree(TreeOp::Move(target, parent, fi))
    }
    "delete" => {
      let target = jsonschema_import_parse_id(
        jsonschema_import_req_string(obj, "target", "tree.delete.target"),
        peers,
        "tree_id",
      )
      OpContent::Tree(TreeOp::Delete(target))
    }
    _ => raise DecodeError("jsonschema_import: unknown tree op type: " + t)
  }
}
