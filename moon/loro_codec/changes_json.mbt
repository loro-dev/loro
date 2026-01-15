///|
fn id_string(id : ID) -> String {
  id.counter().to_string() + "@" + id.peer().to_string()
}

///|
fn idlp_string(id : IdLp) -> String {
  "L" + id.lamport().to_string() + "@" + id.peer().to_string()
}

///|
fn container_type_string(kind : ContainerType) -> String {
  match kind {
    ContainerType::Map => "Map"
    ContainerType::List => "List"
    ContainerType::Text => "Text"
    ContainerType::Tree => "Tree"
    ContainerType::MovableList => "MovableList"
    ContainerType::Counter => "Counter"
    ContainerType::Unknown(k) => "Unknown(" + k.to_string() + ")"
  }
}

///|
fn container_id_string(cid : ContainerID) -> String {
  match cid {
    ContainerID::Root(name, kind) =>
      "cid:root-" + name + ":" + container_type_string(kind)
    ContainerID::Normal(peer, counter, kind) =>
      "cid:" +
      counter.to_string() +
      "@" +
      peer.to_string() +
      ":" +
      container_type_string(kind)
  }
}

///|
fn bytes_hex_upper(bytes : Bytes) -> String {
  let sb = StringBuilder::new()
  let hex : Array[Char] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F',
  ]
  for b in bytes {
    let u = b.to_uint()
    let hi = ((u >> 4) & 0xF).reinterpret_as_int()
    let lo = (u & 0xF).reinterpret_as_int()
    sb.write_char(hex[hi])
    sb.write_char(hex[lo])
  }
  sb.to_string()
}

///|
fn fractional_index_json(fi : FractionalIndex) -> Json {
  Json::string(bytes_hex_upper(fi.bytes()))
}

///|
fn binary_json(bytes : Bytes) -> Json {
  let out : Array[Json] = []
  for b in bytes {
    out.push(Json::number(b.to_uint().to_double()))
  }
  Json::array(out)
}

///|
fn loro_value_json(v : LoroValue, keys : Array[String], depth : Int) -> Json {
  if depth > 1024 {
    return Json::string("<too deep>")
  }
  match v {
    LoroValue::Null => Json::null()
    LoroValue::True => Json::boolean(true)
    LoroValue::False => Json::boolean(false)
    LoroValue::I64(x) => Json::string(x.to_string())
    LoroValue::F64(x) => Json::number(x)
    LoroValue::Str(s) => Json::string(s)
    LoroValue::Binary(b) => binary_json(b)
    LoroValue::List(items) => {
      let out : Array[Json] = []
      for it in items {
        out.push(loro_value_json(it, keys, depth + 1))
      }
      Json::array(out)
    }
    LoroValue::Map(items) => {
      let obj = Map::new(capacity=items.length())
      for pair in items {
        let (key_idx, it) = pair
        let mut key = "<key_idx:" + key_idx.to_string() + ">"
        if key_idx <= 0x7FFF_FFFFUL {
          let i = key_idx.to_int()
          if i >= 0 && i < keys.length() {
            key = keys[i]
          }
        }
        obj[key] = loro_value_json(it, keys, depth + 1)
      }
      Json::object(obj)
    }
    LoroValue::ContainerType(ct) => Json::number(ct.to_uint().to_double())
  }
}

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

///|
fn change_json(change : Change, keys : Array[String]) -> Json {
  let deps : Array[String] = []
  for d in change.deps() {
    deps.push(id_string(d))
  }
  let ops : Array[Json] = []
  for op in change.ops() {
    ops.push(op_json(op, keys))
  }
  {
    "id": id_string(change.id()),
    "timestamp": change.timestamp().to_string(),
    "lamport": change.lamport().to_uint64().to_string(),
    "msg": match change.msg() {
      None => Json::null()
      Some(m) => Json::string(m)
    },
    "deps": deps,
    "ops": Json::array(ops),
  }
}

///|
pub fn decode_fast_updates_changes_json(
  bytes : Bytes,
  validate : Bool,
) -> String raise DecodeError {
  let doc = parse_document(bytes, validate)
  if doc.mode() != 4 {
    raise DecodeError("decode-updates: not a FastUpdates (mode=4) document")
  }
  let blocks = parse_fast_updates_body(doc.body_view())
  let changes : Array[Json] = []
  for b in blocks {
    let decoded = decode_change_block_full(b)
    let keys = decoded.keys()
    for c in decoded.changes() {
      changes.push(change_json(c, keys))
    }
  }
  let root : Json = { "mode": 4, "changes": Json::array(changes) }
  root.stringify(indent=2)
}
