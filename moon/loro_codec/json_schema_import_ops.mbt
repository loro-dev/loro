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
      match t {
        "insert" => {
          let pos = jsonschema_import_req_u32(obj, "pos", "text.insert.pos")
          let text = jsonschema_import_req_string(
            obj, "text", "text.insert.text",
          )
          OpContent::Text(TextOp::Insert(pos, text))
        }
        "delete" => {
          let pos = jsonschema_import_req_int(obj, "pos", "text.delete.pos")
          let len_i64 = jsonschema_import_req_i64(obj, "len", "text.delete.len")
          let start_id = jsonschema_import_parse_id(
            jsonschema_import_req_string(
              obj, "start_id", "text.delete.start_id",
            ),
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
    ContainerType::List =>
      match t {
        "insert" => {
          let pos = jsonschema_import_req_u32(obj, "pos", "list.insert.pos")
          let values_json = jsonschema_import_req_array(
            obj, "value", "list.insert.value",
          )
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
          OpContent::List(ListOp::Insert(pos, values))
        }
        "delete" => {
          let pos = jsonschema_import_req_int(obj, "pos", "list.delete.pos")
          let len_i64 = jsonschema_import_req_i64(obj, "len", "list.delete.len")
          let start_id = jsonschema_import_parse_id(
            jsonschema_import_req_string(
              obj, "start_id", "list.delete.start_id",
            ),
            peers,
            "start_id",
          )
          OpContent::List(ListOp::Delete(pos, len_i64, start_id))
        }
        _ => raise DecodeError("jsonschema_import: unknown list op type: " + t)
      }
    ContainerType::MovableList =>
      match t {
        "insert" => {
          let pos = jsonschema_import_req_u32(obj, "pos", "mlist.insert.pos")
          let values_json = jsonschema_import_req_array(
            obj, "value", "mlist.insert.value",
          )
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
          OpContent::MovableList(MovableListOp::Insert(pos, values))
        }
        "delete" => {
          let pos = jsonschema_import_req_int(obj, "pos", "mlist.delete.pos")
          let len_i64 = jsonschema_import_req_i64(
            obj, "len", "mlist.delete.len",
          )
          let start_id = jsonschema_import_parse_id(
            jsonschema_import_req_string(
              obj, "start_id", "mlist.delete.start_id",
            ),
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
          raise DecodeError(
            "jsonschema_import: unknown movable_list op type: " + t,
          )
      }
    ContainerType::Map =>
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
    ContainerType::Tree =>
      match t {
        "create" => {
          let target = jsonschema_import_parse_id(
            jsonschema_import_req_string(obj, "target", "tree.create.target"),
            peers,
            "tree_id",
          )
          let parent : ID? = match obj.get("parent") {
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
          let parent : ID? = match obj.get("parent") {
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
    ContainerType::Counter =>
      raise DecodeError("jsonschema_import: Counter ops are not supported")
    ContainerType::Unknown(_) =>
      raise DecodeError(
        "jsonschema_import: Unknown container type is not supported",
      )
  }
}
