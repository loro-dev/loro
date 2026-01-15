// -----------------------------------------------------------------------------
// JsonSchema -> FastUpdates (encode)
// -----------------------------------------------------------------------------

///|
fn jsonschema_import_parse_uint64_decimal(
  s : String,
  what : String,
) -> UInt64 raise DecodeError {
  @strconv.parse_uint64(s[:]) catch {
    @strconv.StrConvError(err) =>
      raise DecodeError("jsonschema_import: invalid " + what + ": " + err)
  }
}

///|
fn jsonschema_import_parse_int64_decimal(
  s : String,
  what : String,
) -> Int64 raise DecodeError {
  @strconv.parse_int64(s[:]) catch {
    @strconv.StrConvError(err) =>
      raise DecodeError("jsonschema_import: invalid " + what + ": " + err)
  }
}

///|
fn jsonschema_import_parse_int_decimal(
  s : String,
  what : String,
) -> Int raise DecodeError {
  @strconv.parse_int(s[:]) catch {
    @strconv.StrConvError(err) =>
      raise DecodeError("jsonschema_import: invalid " + what + ": " + err)
  }
}

///|
fn jsonschema_import_expect_object(
  v : Json,
  what : String,
) -> Map[String, Json] raise DecodeError {
  match v {
    Json::Object(obj) => obj
    _ => raise DecodeError("jsonschema_import: expected object for " + what)
  }
}

///|
fn jsonschema_import_expect_array(
  v : Json,
  what : String,
) -> Array[Json] raise DecodeError {
  match v {
    Json::Array(arr) => arr
    _ => raise DecodeError("jsonschema_import: expected array for " + what)
  }
}

///|
fn jsonschema_import_expect_string(
  v : Json,
  what : String,
) -> String raise DecodeError {
  match v {
    Json::String(s) => s
    _ => raise DecodeError("jsonschema_import: expected string for " + what)
  }
}

///|
fn jsonschema_import_expect_number(
  v : Json,
  what : String,
) -> (Double, String?) raise DecodeError {
  match v {
    Json::Number(n, repr~) => (n, repr)
    _ => raise DecodeError("jsonschema_import: expected number for " + what)
  }
}

///|
fn jsonschema_import_req(
  obj : Map[String, Json],
  key : String,
  what : String,
) -> Json raise DecodeError {
  match obj.get(key) {
    Some(v) => v
    None => raise DecodeError("jsonschema_import: missing " + what)
  }
}

///|
fn jsonschema_import_req_string(
  obj : Map[String, Json],
  key : String,
  what : String,
) -> String raise DecodeError {
  jsonschema_import_expect_string(jsonschema_import_req(obj, key, what), what)
}

///|
fn jsonschema_import_req_array(
  obj : Map[String, Json],
  key : String,
  what : String,
) -> Array[Json] raise DecodeError {
  jsonschema_import_expect_array(jsonschema_import_req(obj, key, what), what)
}

///|
fn jsonschema_import_req_int(
  obj : Map[String, Json],
  key : String,
  what : String,
) -> Int raise DecodeError {
  jsonschema_import_number_to_int(jsonschema_import_req(obj, key, what), what)
}

///|
fn jsonschema_import_req_i64(
  obj : Map[String, Json],
  key : String,
  what : String,
) -> Int64 raise DecodeError {
  jsonschema_import_number_to_i64(jsonschema_import_req(obj, key, what), what)
}

///|
fn jsonschema_import_req_u32(
  obj : Map[String, Json],
  key : String,
  what : String,
) -> UInt raise DecodeError {
  jsonschema_import_number_to_u32(jsonschema_import_req(obj, key, what), what)
}

///|
fn jsonschema_import_number_to_int(
  v : Json,
  what : String,
) -> Int raise DecodeError {
  let (n, repr) = jsonschema_import_expect_number(v, what)
  // Prefer repr for integer decoding to avoid float rounding.
  match repr {
    Some(s) => jsonschema_import_parse_int_decimal(s, what)
    None => {
      // Best-effort: require it to be an integer.
      let i = n.to_int()
      if i.to_double() != n {
        raise DecodeError(
          "jsonschema_import: expected integer number for " + what,
        )
      }
      i
    }
  }
}

///|
fn jsonschema_import_number_to_i64(
  v : Json,
  what : String,
) -> Int64 raise DecodeError {
  let (n, repr) = jsonschema_import_expect_number(v, what)
  match repr {
    Some(s) => jsonschema_import_parse_int64_decimal(s, what)
    None => {
      let i = n.to_int64()
      if i.to_double() != n {
        raise DecodeError(
          "jsonschema_import: expected integer number for " + what,
        )
      }
      i
    }
  }
}

///|
fn jsonschema_import_number_to_u32(
  v : Json,
  what : String,
) -> UInt raise DecodeError {
  let i = jsonschema_import_number_to_int(v, what)
  if i < 0 {
    raise DecodeError(
      "jsonschema_import: expected non-negative integer for " + what,
    )
  }
  i.reinterpret_as_uint()
}

///|
fn jsonschema_import_container_type_from_string(
  s : String,
) -> ContainerType raise DecodeError {
  match s {
    "Map" => ContainerType::Map
    "List" => ContainerType::List
    "Text" => ContainerType::Text
    "Tree" => ContainerType::Tree
    "MovableList" => ContainerType::MovableList
    "Counter" => ContainerType::Counter
    _ => raise DecodeError("jsonschema_import: unknown container type: " + s)
  }
}

///|
fn jsonschema_import_parse_id_raw(
  s : String,
  what : String,
) -> (Int, UInt64) raise DecodeError {
  let parts = s.split("@").collect()
  if parts.length() != 2 {
    raise DecodeError("jsonschema_import: invalid " + what + " id: " + s)
  }
  let counter = jsonschema_import_parse_int_decimal(
    parts[0].to_string(),
    what + ".counter",
  )
  let peer = jsonschema_import_parse_uint64_decimal(
    parts[1].to_string(),
    what + ".peer",
  )
  (counter, peer)
}

///|
fn jsonschema_import_parse_id(
  s : String,
  peers : Array[UInt64]?,
  what : String,
) -> ID raise DecodeError {
  let (counter, peer_or_idx) = jsonschema_import_parse_id_raw(s, what)
  let peer = match peers {
    None => peer_or_idx
    Some(ps) => {
      if peer_or_idx > 0x7FFF_FFFFUL {
        raise DecodeError("jsonschema_import: peer_idx too large for " + what)
      }
      let i = peer_or_idx.to_int()
      if i < 0 || i >= ps.length() {
        raise DecodeError(
          "jsonschema_import: peer_idx out of range for " + what,
        )
      }
      ps[i]
    }
  }
  ID::new(peer, counter)
}

///|
fn jsonschema_import_parse_idlp(
  s : String,
  peers : Array[UInt64]?,
  what : String,
) -> IdLp raise DecodeError {
  if !s.has_prefix("L") {
    raise DecodeError("jsonschema_import: invalid " + what + " idlp: " + s)
  }
  let rest = (s[1:] catch {
    _ => raise DecodeError("jsonschema_import: invalid " + what + " idlp: " + s)
  }).to_string()
  let (lamport_i, peer_or_idx) = jsonschema_import_parse_id_raw(rest, what)
  if lamport_i < 0 {
    raise DecodeError("jsonschema_import: negative lamport for " + what)
  }
  let peer = match peers {
    None => peer_or_idx
    Some(ps) => {
      if peer_or_idx > 0x7FFF_FFFFUL {
        raise DecodeError("jsonschema_import: peer_idx too large for " + what)
      }
      let i = peer_or_idx.to_int()
      if i < 0 || i >= ps.length() {
        raise DecodeError(
          "jsonschema_import: peer_idx out of range for " + what,
        )
      }
      ps[i]
    }
  }
  IdLp::new(peer, lamport_i.reinterpret_as_uint())
}

///|
fn jsonschema_import_parse_container_id(
  s : String,
  peers : Array[UInt64]?,
) -> ContainerID raise DecodeError {
  if !s.has_prefix("cid:") {
    raise DecodeError("jsonschema_import: invalid container id: " + s)
  }
  if s.has_prefix("cid:root-") {
    // cid:root-${name}:${ContainerType}
    let rest = (s["cid:root-".length():] catch {
      _ =>
        raise DecodeError("jsonschema_import: invalid root container id: " + s)
    }).to_string()
    let parts = rest.split(":").collect()
    if parts.length() != 2 {
      raise DecodeError("jsonschema_import: invalid root container id: " + s)
    }
    let name = parts[0].to_string()
    let kind = jsonschema_import_container_type_from_string(
      parts[1].to_string(),
    )
    return ContainerID::Root(name, kind)
  }

  // cid:${counter}@${peer}:${ContainerType}
  let rest = (s["cid:".length():] catch {
    _ => raise DecodeError("jsonschema_import: invalid container id: " + s)
  }).to_string()
  let parts = rest.split(":").collect()
  if parts.length() != 2 {
    raise DecodeError("jsonschema_import: invalid container id: " + s)
  }
  let id_part = parts[0].to_string()
  let kind = jsonschema_import_container_type_from_string(parts[1].to_string())
  let (counter, peer_or_idx) = jsonschema_import_parse_id_raw(
    id_part, "container_id",
  )
  let peer = match peers {
    None => peer_or_idx
    Some(ps) => {
      if peer_or_idx > 0x7FFF_FFFFUL {
        raise DecodeError("jsonschema_import: container peer_idx too large")
      }
      let i = peer_or_idx.to_int()
      if i < 0 || i >= ps.length() {
        raise DecodeError("jsonschema_import: container peer_idx out of range")
      }
      ps[i]
    }
  }
  ContainerID::Normal(peer, counter, kind)
}

///|
fn jsonschema_import_hex_nibble(c : UInt16) -> Int raise DecodeError {
  let x = c.to_int()
  if x >= '0'.to_int() && x <= '9'.to_int() {
    x - '0'.to_int()
  } else if x >= 'a'.to_int() && x <= 'f'.to_int() {
    10 + (x - 'a'.to_int())
  } else if x >= 'A'.to_int() && x <= 'F'.to_int() {
    10 + (x - 'A'.to_int())
  } else {
    raise DecodeError("jsonschema_import: invalid hex char")
  }
}

///|
fn jsonschema_import_hex_to_bytes(hex : String) -> Bytes raise DecodeError {
  if hex.length() % 2 != 0 {
    raise DecodeError("jsonschema_import: invalid hex length")
  }
  let out_len = hex.length() / 2
  Bytes::makei(out_len, i => {
    let hi = jsonschema_import_hex_nibble(hex[2 * i])
    let lo = jsonschema_import_hex_nibble(hex[2 * i + 1])
    ((hi << 4) | lo).to_byte()
  })
}

///|
fn jsonschema_import_loro_value(
  v : Json,
  op_id : ID,
  keys : Array[String],
  key_to_idx : @hashmap.HashMap[String, UInt64],
  peers : Array[UInt64]?,
) -> LoroValue raise DecodeError {
  match v {
    Json::Null => LoroValue::Null
    Json::True => LoroValue::True
    Json::False => LoroValue::False
    Json::Number(_n, repr~) =>
      // Decide int vs float by lexical repr when available.
      match repr {
        Some(s) =>
          if s.contains(".") || s.contains("e") || s.contains("E") {
            LoroValue::F64(_n)
          } else {
            LoroValue::I64(
              jsonschema_import_parse_int64_decimal(s, "loro_value.i64"),
            )
          }
        None => LoroValue::F64(_n)
      }
    Json::String(s) =>
      if s.has_prefix(LORO_CONTAINER_ID_PREFIX) {
        let cid_str = (s[LORO_CONTAINER_ID_PREFIX.length():] catch {
          _ =>
            raise DecodeError(
              "jsonschema_import: invalid container value string",
            )
        }).to_string()
        let cid = jsonschema_import_parse_container_id(cid_str, peers)
        match cid {
          ContainerID::Root(_, _kind) =>
            // Root container ids are not representable in binary value encoding; reject for now.
            raise DecodeError(
              "jsonschema_import: root container value is not supported",
            )
          ContainerID::Normal(peer, counter, kind) => {
            // Binary value encoding reconstructs container id from op_id + type.
            if peer != op_id.peer() || counter != op_id.counter() {
              raise DecodeError(
                "jsonschema_import: container value id mismatch",
              )
            }
            LoroValue::ContainerType(container_type_to_u8(kind))
          }
        }
      } else {
        LoroValue::Str(s)
      }
    Json::Array(arr) => {
      let out : Array[LoroValue] = []
      for i in 0..<arr.length() {
        out.push(
          jsonschema_import_loro_value(
            arr[i],
            id_inc(op_id, i),
            keys,
            key_to_idx,
            peers,
          ),
        )
      }
      LoroValue::List(out)
    }
    Json::Object(obj) => {
      let out : Array[(UInt64, LoroValue)] = []
      for k, vv in obj {
        let idx = register_key(keys, key_to_idx, k)
        out.push(
          (
            idx,
            jsonschema_import_loro_value(vv, op_id, keys, key_to_idx, peers),
          ),
        )
      }
      LoroValue::Map(out)
    }
  }
}

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
            None => Option::None
            Some(Json::Null) => Option::None
            Some(vp) =>
              Option::Some(
                jsonschema_import_parse_id(
                  jsonschema_import_expect_string(vp, "tree.parent"),
                  peers,
                  "tree_id",
                ),
              )
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
            None => Option::None
            Some(Json::Null) => Option::None
            Some(vp) =>
              Option::Some(
                jsonschema_import_parse_id(
                  jsonschema_import_expect_string(vp, "tree.parent"),
                  peers,
                  "tree_id",
                ),
              )
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
    _ =>
      raise DecodeError(
        "jsonschema_import: unsupported container type in op content",
      )
  }
}

///|
fn jsonschema_import_parse_change(
  v : Json,
  keys : Array[String],
  key_to_idx : @hashmap.HashMap[String, UInt64],
  peers : Array[UInt64]?,
) -> Change raise DecodeError {
  let obj = jsonschema_import_expect_object(v, "change")
  let id_str = jsonschema_import_req_string(obj, "id", "change.id")
  let id = jsonschema_import_parse_id(id_str, peers, "change.id")
  let timestamp = jsonschema_import_req_i64(
    obj, "timestamp", "change.timestamp",
  )
  let lamport_u32 = jsonschema_import_req_u32(obj, "lamport", "change.lamport")
  let msg : String? = match obj.get("msg") {
    None => None
    Some(Json::Null) => None
    Some(vs) => Some(jsonschema_import_expect_string(vs, "change.msg"))
  }
  let deps_json = jsonschema_import_req_array(obj, "deps", "change.deps")
  let deps : Array[ID] = []
  for i in 0..<deps_json.length() {
    let dep_s = jsonschema_import_expect_string(
      deps_json[i],
      "change.deps[" + i.to_string() + "]",
    )
    deps.push(jsonschema_import_parse_id(dep_s, peers, "change.dep"))
  }
  let c = Change::new(id, timestamp, deps, lamport_u32, msg)
  let ops_json = jsonschema_import_req_array(obj, "ops", "change.ops")
  for i in 0..<ops_json.length() {
    let op_obj = jsonschema_import_expect_object(ops_json[i], "op")
    let container_str = jsonschema_import_req_string(
      op_obj, "container", "op.container",
    )
    let container = jsonschema_import_parse_container_id(container_str, peers)
    let counter = jsonschema_import_req_int(op_obj, "counter", "op.counter")
    let op_id = ID::new(id.peer(), counter)
    let content = jsonschema_import_parse_op_content(
      container,
      jsonschema_import_req(op_obj, "content", "op.content"),
      op_id,
      keys,
      key_to_idx,
      peers,
    )
    let len = jsonschema_import_op_len(content)
    c.ops().push(Op::new(container, counter, len, content))
  }
  c
}

///|
fn jsonschema_import_sort_and_validate_changes(
  changes : Array[Change],
) -> Array[Change] raise DecodeError {
  if changes.length() == 0 {
    raise DecodeError("jsonschema_import: empty changes")
  }
  let peer0 = changes[0].id().peer()
  for c in changes {
    if c.id().peer() != peer0 {
      raise DecodeError("jsonschema_import: mixed peers in one block")
    }
  }

  // Sort by change start counter.
  changes.sort_by_key(c => c.id().counter())
  let mut expected = changes[0].id().counter()
  for c in changes {
    if c.id().counter() != expected {
      raise DecodeError("jsonschema_import: change counters not contiguous")
    }
    let ops = c.ops()
    ops.sort_by_key(op => op.counter())
    let mut op_expected = expected
    let mut atom_len : Int64 = 0
    for op in ops {
      if op.counter() != op_expected {
        raise DecodeError("jsonschema_import: op counters not contiguous")
      }
      let l = op.len().to_int64()
      atom_len = atom_len + l
      op_expected = op_expected + l.to_int()
    }
    expected = expected + atom_len.to_int()
  }
  changes
}

///|
/// Encode a JsonSchema JSON string into a FastUpdates (mode=4) document blob.
///
/// This is intended for tooling/tests (JsonSchema <-> binary). It only supports the
/// subset of JsonSchema that corresponds to current containers (Map/List/Text/Tree/MovableList)
/// and does not support UnknownOp yet.
pub fn encode_fast_updates_from_json_schema(
  json : String,
  validate : Bool,
) -> Bytes raise DecodeError {
  let root = @json.parse(json[:]) catch {
    _ => raise DecodeError("jsonschema_import: invalid json")
  }
  let obj = jsonschema_import_expect_object(root, "root")

  // peers: optional; when present, ids use peer indices.
  let peers : Array[UInt64]? = match obj.get("peers") {
    None => None
    Some(Json::Null) => None
    Some(v) => {
      let arr = jsonschema_import_expect_array(v, "peers")
      let ps : Array[UInt64] = []
      for i in 0..<arr.length() {
        let s = jsonschema_import_expect_string(
          arr[i],
          "peers[" + i.to_string() + "]",
        )
        ps.push(jsonschema_import_parse_uint64_decimal(s, "peer_id"))
      }
      Some(ps)
    }
  }

  // Parse all changes first.
  let keys : Array[String] = []
  let key_to_idx : @hashmap.HashMap[String, UInt64] = @hashmap.new(capacity=64)
  let changes_json = jsonschema_import_req_array(obj, "changes", "changes")
  let all_changes : Array[Change] = []
  for i in 0..<changes_json.length() {
    all_changes.push(
      jsonschema_import_parse_change(changes_json[i], keys, key_to_idx, peers),
    )
  }
  if all_changes.length() == 0 {
    raise DecodeError("jsonschema_import: empty changes")
  }

  // Group changes by actual peer.
  let by_peer : @hashmap.HashMap[UInt64, Array[Change]] = @hashmap.new(
    capacity=16,
  )
  for c in all_changes {
    let p = c.id().peer()
    match by_peer.get(p) {
      None => {
        let arr : Array[Change] = []
        arr.push(c)
        by_peer.set(p, arr)
      }
      Some(arr) => arr.push(c)
    }
  }
  let peer_entries = by_peer.to_array()
  let peer_ids : Array[UInt64] = []
  for pair in peer_entries {
    let (p, _arr) = pair
    peer_ids.push(p)
  }
  peer_ids.sort()
  let blocks : Array[Bytes] = []
  for peer in peer_ids {
    let arr = match by_peer.get(peer) {
      None => []
      Some(x) => x
    }
    let sorted_changes = jsonschema_import_sort_and_validate_changes(arr)
    let block : DecodedChangeBlock = {
      peers: [],
      keys,
      cids: [],
      positions: [],
      changes: sorted_changes,
    }
    let b = encode_change_block(block) catch {
      EncodeError(msg) =>
        raise DecodeError(
          "jsonschema_import: encode change block failed: " + msg,
        )
    }
    if validate {
      let _ = decode_change_block(b[:])

    }
    blocks.push(b)
  }
  let body = encode_fast_updates_body(blocks)
  encode_document(4, body)
}
