///|
fn jsonschema_import_parse_text_op_content(
  obj : Map[String, Json],
  t : String,
  op_id : ID,
  keys : Array[String],
  key_to_idx : @hashmap.HashMap[String, UInt64],
  peers : Array[UInt64]?,
) -> OpContent raise DecodeError {
  match t {
    "insert" => {
      let pos = jsonschema_import_req_u32(obj, "pos", "text.insert.pos")
      let text = jsonschema_import_req_string(obj, "text", "text.insert.text")
      OpContent::Text(TextOp::Insert(pos, text))
    }
    "delete" => {
      let pos = jsonschema_import_req_int(obj, "pos", "text.delete.pos")
      let len_i64 = jsonschema_import_req_i64(obj, "len", "text.delete.len")
      let start_id = jsonschema_import_parse_id(
        jsonschema_import_req_string(obj, "start_id", "text.delete.start_id"),
        peers,
        "start_id",
      )
      OpContent::Text(TextOp::Delete(pos, len_i64, start_id))
    }
    "mark" => {
      let start = jsonschema_import_req_u32(obj, "start", "text.mark.start")
      let end = jsonschema_import_req_u32(obj, "end", "text.mark.end")
      let style_key = jsonschema_import_req_string(
        obj, "style_key", "text.mark.style_key",
      )
      let style_val = jsonschema_import_loro_value(
        jsonschema_import_req(obj, "style_value", "text.mark.style_value"),
        op_id,
        keys,
        key_to_idx,
        peers,
      )
      let info_i = jsonschema_import_req_int(obj, "info", "text.mark.info")
      if info_i < 0 || info_i > 255 {
        raise DecodeError("jsonschema_import: invalid text.mark.info")
      }
      OpContent::Text(
        TextOp::Mark(start, end, style_key, style_val, info_i.to_byte()),
      )
    }
    "mark_end" => OpContent::Text(TextOp::MarkEnd)
    _ => raise DecodeError("jsonschema_import: unknown text op type: " + t)
  }
}
