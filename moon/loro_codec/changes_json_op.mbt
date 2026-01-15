///|
fn op_content_json(content : OpContent, keys : Array[String]) -> Json {
  match content {
    OpContent::Map(MapOp::Insert(k, v)) =>
      {
        "Map": { "Insert": { "key": k, "value": loro_value_json(v, keys, 0) } },
      }
    OpContent::Map(MapOp::Delete(k)) => { "Map": { "Delete": { "key": k } } }
    OpContent::List(ListOp::Insert(pos, values)) => {
      let out : Array[Json] = []
      for v in values {
        out.push(loro_value_json(v, keys, 0))
      }
      {
        "List": {
          "Insert": {
            "pos": pos.reinterpret_as_int(),
            "values": Json::array(out),
          },
        },
      }
    }
    OpContent::List(ListOp::Delete(pos, len, start_id)) =>
      {
        "List": {
          "Delete": {
            "pos": pos,
            "len": len.to_string(),
            "start_id": id_string(start_id),
          },
        },
      }
    OpContent::MovableList(MovableListOp::Insert(pos, values)) => {
      let out : Array[Json] = []
      for v in values {
        out.push(loro_value_json(v, keys, 0))
      }
      {
        "MovableList": {
          "Insert": {
            "pos": pos.reinterpret_as_int(),
            "values": Json::array(out),
          },
        },
      }
    }
    OpContent::MovableList(MovableListOp::Delete(pos, len, start_id)) =>
      {
        "MovableList": {
          "Delete": {
            "pos": pos,
            "len": len.to_string(),
            "start_id": id_string(start_id),
          },
        },
      }
    OpContent::MovableList(MovableListOp::Move(from, to, elem_id)) =>
      {
        "MovableList": {
          "Move": {
            "from": from.reinterpret_as_int(),
            "to": to.reinterpret_as_int(),
            "elem_id": idlp_string(elem_id),
          },
        },
      }
    OpContent::MovableList(MovableListOp::Set(elem_id, value)) =>
      {
        "MovableList": {
          "Set": {
            "elem_id": idlp_string(elem_id),
            "value": loro_value_json(value, keys, 0),
          },
        },
      }
    OpContent::Text(TextOp::Insert(pos, text)) =>
      {
        "Text": { "Insert": { "pos": pos.reinterpret_as_int(), "text": text } },
      }
    OpContent::Text(TextOp::Delete(pos, len, start_id)) =>
      {
        "Text": {
          "Delete": {
            "pos": pos,
            "len": len.to_string(),
            "start_id": id_string(start_id),
          },
        },
      }
    OpContent::Text(TextOp::Mark(start, end, key, value, info)) =>
      {
        "Text": {
          "Mark": {
            "start": start.reinterpret_as_int(),
            "end": end.reinterpret_as_int(),
            "key": key,
            "value": loro_value_json(value, keys, 0),
            "info": info.to_uint().reinterpret_as_int(),
          },
        },
      }
    OpContent::Text(TextOp::MarkEnd) => { "Text": { "MarkEnd": Json::null() } }
    OpContent::Tree(TreeOp::Create(target, parent, fi)) =>
      {
        "Tree": {
          "Create": {
            "target": id_string(target),
            "parent": match parent {
              None => Json::null()
              Some(p) => Json::string(id_string(p))
            },
            "fractional_index": fractional_index_json(fi),
          },
        },
      }
    OpContent::Tree(TreeOp::Move(target, parent, fi)) =>
      {
        "Tree": {
          "Move": {
            "target": id_string(target),
            "parent": match parent {
              None => Json::null()
              Some(p) => Json::string(id_string(p))
            },
            "fractional_index": fractional_index_json(fi),
          },
        },
      }
    OpContent::Tree(TreeOp::Delete(target)) =>
      { "Tree": { "Delete": { "target": id_string(target) } } }
    OpContent::Future(FutureOp::Unknown(prop, _raw)) =>
      { "Future": { "Unknown": { "prop": prop } } }
  }
}

///|
fn op_json(op : Op, keys : Array[String]) -> Json {
  {
    "container": container_id_string(op.container()),
    "counter": op.counter(),
    "len": op.len().to_uint64().to_string(),
    "content": op_content_json(op.content(), keys),
  }
}
