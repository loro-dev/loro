const JSON_SCHEMA_VERSION: Int = 1
const LORO_CONTAINER_ID_PREFIX: String = "🦜:"

fn jsonschema_bytes_hex_upper(bytes: Bytes) -> String {
  let sb = StringBuilder::new()
  let hex: Array[Char] = [
    '0',
    '1',
    '2',
    '3',
    '4',
    '5',
    '6',
    '7',
    '8',
    '9',
    'A',
    'B',
    'C',
    'D',
    'E',
    'F',
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

fn jsonschema_register_peer(
  peers: Array[UInt64],
  peer_to_idx: @hashmap.HashMap[UInt64, UInt64],
  peer: UInt64,
) -> UInt64 {
  match peer_to_idx.get(peer) {
    Some(idx) => idx
    None => {
      let idx = peers.length().to_uint64()
      peers.push(peer)
      peer_to_idx.set(peer, idx)
      idx
    }
  }
}

fn id_key_actual(id: ID) -> String {
  id.counter().to_string() + "@" + id.peer().to_string()
}

fn id_string_with_peer_index(
  id: ID,
  peers: Array[UInt64],
  peer_to_idx: @hashmap.HashMap[UInt64, UInt64],
) -> String {
  let idx = jsonschema_register_peer(peers, peer_to_idx, id.peer())
  id.counter().to_string() + "@" + idx.to_string()
}

fn idlp_string_with_peer_index(
  idlp: IdLp,
  peers: Array[UInt64],
  peer_to_idx: @hashmap.HashMap[UInt64, UInt64],
) -> String {
  let idx = jsonschema_register_peer(peers, peer_to_idx, idlp.peer())
  "L" + idlp.lamport().to_uint64().to_string() + "@" + idx.to_string()
}

fn jsonschema_container_type_string(kind: ContainerType) -> String {
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

fn container_id_string_with_peer_index(
  cid: ContainerID,
  peers: Array[UInt64],
  peer_to_idx: @hashmap.HashMap[UInt64, UInt64],
) -> String {
  match cid {
    ContainerID::Root(name, kind) => "cid:root-" + name + ":" + jsonschema_container_type_string(kind)
    ContainerID::Normal(peer, counter, kind) => {
      let idx = jsonschema_register_peer(peers, peer_to_idx, peer)
      "cid:" + counter.to_string() + "@" + idx.to_string() + ":" + jsonschema_container_type_string(kind)
    }
  }
}

fn id_inc(id: ID, delta: Int) -> ID raise DecodeError {
  let mut counter64 = id.counter().to_int64()
  counter64 = counter64 + delta.to_int64()
  if counter64 < -2147483648L || counter64 > 2147483647L {
    raise DecodeError("jsonschema: id counter overflow")
  }
  ID::new(id.peer(), counter64.to_int())
}

fn jsonschema_binary_json(bytes: Bytes) -> Json {
  let out: Array[Json] = []
  for b in bytes {
    out.push(Json::number(b.to_uint().to_double()))
  }
  Json::array(out)
}

fn jsonschema_number_i64(i: Int64) -> Json {
  Json::number(i.to_double(), repr=i.to_string())
}

fn jsonschema_number_u64(u: UInt64) -> Json {
  Json::number(u.to_double(), repr=u.to_string())
}

fn jsonschema_number_int(i: Int) -> Json {
  Json::number(i.to_double(), repr=i.to_string())
}

fn loro_value_json_schema(
  v: LoroValue,
  keys: Array[String],
  id: ID,
  peers: Array[UInt64],
  peer_to_idx: @hashmap.HashMap[UInt64, UInt64],
  depth: Int,
) -> Json raise DecodeError {
  if depth > 1024 {
    raise DecodeError("jsonschema: value too deep")
  }

  match v {
    LoroValue::Null => Json::null()
    LoroValue::True => Json::boolean(true)
    LoroValue::False => Json::boolean(false)
    LoroValue::I64(x) => jsonschema_number_i64(x)
    LoroValue::F64(x) => Json::number(x)
    LoroValue::Str(s) => Json::string(s)
    LoroValue::Binary(b) => jsonschema_binary_json(b)
    LoroValue::List(items) => {
      let out: Array[Json] = []
      for i in 0..<items.length() {
        let child_id = id_inc(id, i)
        out.push(loro_value_json_schema(items[i], keys, child_id, peers, peer_to_idx, depth + 1))
      }
      Json::array(out)
    }
    LoroValue::Map(items) => {
      let obj = Map::new(capacity=items.length())
      for pair in items {
        let (key_idx, value) = pair
        let mut key = "<key_idx:" + key_idx.to_string() + ">"
        if key_idx <= 0x7FFF_FFFFUL {
          let i = key_idx.to_int()
          if i >= 0 && i < keys.length() {
            key = keys[i]
          }
        }
        obj[key] = loro_value_json_schema(value, keys, id, peers, peer_to_idx, depth + 1)
      }
      Json::object(obj)
    }
    LoroValue::ContainerType(ct) => {
      let kind = container_type_from_u8(ct)
      let peer_idx = jsonschema_register_peer(peers, peer_to_idx, id.peer())
      let cid =
        "cid:" + id.counter().to_string() + "@" + peer_idx.to_string() + ":" +
        jsonschema_container_type_string(kind)
      Json::string(LORO_CONTAINER_ID_PREFIX + cid)
    }
  }
}

fn op_content_json_schema(
  content: OpContent,
  keys: Array[String],
  op_id: ID,
  peers: Array[UInt64],
  peer_to_idx: @hashmap.HashMap[UInt64, UInt64],
) -> Json raise DecodeError {
  match content {
    OpContent::Text(TextOp::Insert(pos, text)) => {
      { "type": "insert", "pos": pos.reinterpret_as_int(), "text": text }
    }
    OpContent::Text(TextOp::Delete(pos, len, start_id)) => {
      {
        "type": "delete",
        "pos": pos,
        "len": jsonschema_number_i64(len),
        "start_id": id_string_with_peer_index(start_id, peers, peer_to_idx),
      }
    }
    OpContent::Text(TextOp::Mark(start, end, style_key, style_value, info)) => {
      {
        "type": "mark",
        "start": start.reinterpret_as_int(),
        "end": end.reinterpret_as_int(),
        "style_key": style_key,
        "style_value": loro_value_json_schema(style_value, keys, op_id, peers, peer_to_idx, 0),
        "info": info.to_uint().reinterpret_as_int(),
      }
    }
    OpContent::Text(TextOp::MarkEnd) => { "type": "mark_end" }

    OpContent::List(ListOp::Insert(pos, values)) => {
      let out: Array[Json] = []
      for i in 0..<values.length() {
        let child_id = id_inc(op_id, i)
        out.push(loro_value_json_schema(values[i], keys, child_id, peers, peer_to_idx, 0))
      }
      { "type": "insert", "pos": pos.reinterpret_as_int(), "value": Json::array(out) }
    }
    OpContent::List(ListOp::Delete(pos, len, start_id)) => {
      {
        "type": "delete",
        "pos": pos,
        "len": jsonschema_number_i64(len),
        "start_id": id_string_with_peer_index(start_id, peers, peer_to_idx),
      }
    }

    OpContent::MovableList(MovableListOp::Insert(pos, values)) => {
      let out: Array[Json] = []
      for i in 0..<values.length() {
        let child_id = id_inc(op_id, i)
        out.push(loro_value_json_schema(values[i], keys, child_id, peers, peer_to_idx, 0))
      }
      { "type": "insert", "pos": pos.reinterpret_as_int(), "value": Json::array(out) }
    }
    OpContent::MovableList(MovableListOp::Delete(pos, len, start_id)) => {
      {
        "type": "delete",
        "pos": pos,
        "len": jsonschema_number_i64(len),
        "start_id": id_string_with_peer_index(start_id, peers, peer_to_idx),
      }
    }
    OpContent::MovableList(MovableListOp::Move(from, to, elem_id)) => {
      {
        "type": "move",
        "from": from.reinterpret_as_int(),
        "to": to.reinterpret_as_int(),
        "elem_id": idlp_string_with_peer_index(elem_id, peers, peer_to_idx),
      }
    }
    OpContent::MovableList(MovableListOp::Set(elem_id, value)) => {
      {
        "type": "set",
        "elem_id": idlp_string_with_peer_index(elem_id, peers, peer_to_idx),
        "value": loro_value_json_schema(value, keys, op_id, peers, peer_to_idx, 0),
      }
    }

    OpContent::Map(MapOp::Insert(key, value)) => {
      {
        "type": "insert",
        "key": key,
        "value": loro_value_json_schema(value, keys, op_id, peers, peer_to_idx, 0),
      }
    }
    OpContent::Map(MapOp::Delete(key)) => { "type": "delete", "key": key }

    OpContent::Tree(TreeOp::Create(target, parent, fi)) => {
      {
        "type": "create",
        "target": id_string_with_peer_index(target, peers, peer_to_idx),
        "parent": match parent {
          None => Json::null()
          Some(p) => Json::string(id_string_with_peer_index(p, peers, peer_to_idx))
        },
        "fractional_index": jsonschema_bytes_hex_upper(fi.bytes()),
      }
    }
    OpContent::Tree(TreeOp::Move(target, parent, fi)) => {
      {
        "type": "move",
        "target": id_string_with_peer_index(target, peers, peer_to_idx),
        "parent": match parent {
          None => Json::null()
          Some(p) => Json::string(id_string_with_peer_index(p, peers, peer_to_idx))
        },
        "fractional_index": jsonschema_bytes_hex_upper(fi.bytes()),
      }
    }
    OpContent::Tree(TreeOp::Delete(target)) => {
      { "type": "delete", "target": id_string_with_peer_index(target, peers, peer_to_idx) }
    }

    OpContent::Future(FutureOp::Unknown(prop, _raw)) => {
      // Best-effort: keep unknown ops opaque. This is only used when container type is Counter/Unknown.
      { "type": "unknown", "prop": prop, "value_type": "unknown", "value": Json::null() }
    }
  }
}

fn op_json_schema(
  op: Op,
  keys: Array[String],
  change_peer: UInt64,
  peers: Array[UInt64],
  peer_to_idx: @hashmap.HashMap[UInt64, UInt64],
) -> Json raise DecodeError {
  let op_id = ID::new(change_peer, op.counter())
  {
    "container": container_id_string_with_peer_index(op.container(), peers, peer_to_idx),
    "content": op_content_json_schema(op.content(), keys, op_id, peers, peer_to_idx),
    "counter": jsonschema_number_int(op.counter()),
  }
}

fn change_json_schema(
  change: Change,
  keys: Array[String],
  peers: Array[UInt64],
  peer_to_idx: @hashmap.HashMap[UInt64, UInt64],
) -> Json raise DecodeError {
  let deps: Array[String] = []
  for d in change.deps() {
    deps.push(id_string_with_peer_index(d, peers, peer_to_idx))
  }

  let ops: Array[Json] = []
  for op in change.ops() {
    ops.push(op_json_schema(op, keys, change.id().peer(), peers, peer_to_idx))
  }

  {
    "id": id_string_with_peer_index(change.id(), peers, peer_to_idx),
    "timestamp": jsonschema_number_i64(change.timestamp()),
    "deps": deps,
    "lamport": jsonschema_number_u64(change.lamport().to_uint64()),
    "msg": match change.msg() {
      None => Json::null()
      Some(m) => Json::string(m)
    },
    "ops": Json::array(ops),
  }
}

/// Export a FastUpdates (mode=4) document blob into JSON schema format as described in
/// `docs/JsonSchema.md` (peer-compressed).
pub fn export_json_schema_from_fast_updates(bytes: Bytes, validate: Bool) -> String raise DecodeError {
  let doc = parse_document(bytes, validate)
  if doc.mode() != 4 {
    raise DecodeError("jsonschema: not a FastUpdates (mode=4) document")
  }

  // Decode blocks first so we can detect which deps are outside the exported set.
  let blocks = parse_fast_updates_body(doc.body_view())
  let decoded_blocks: Array[DecodedChangeBlock] = []
  for b in blocks {
    decoded_blocks.push(decode_change_block_full(b))
  }

  let included: @hashmap.HashMap[String, Bool] = @hashmap.new(capacity=1024)
  for blk in decoded_blocks {
    for c in blk.changes() {
      included.set(id_key_actual(c.id()), true)
    }
  }

  // Derive `start_version` by collecting deps that are outside the exported set.
  let external_deps_max: @hashmap.HashMap[UInt64, Int] = @hashmap.new(capacity=64)
  for blk in decoded_blocks {
    for c in blk.changes() {
      for dep in c.deps() {
        if !included.contains(id_key_actual(dep)) {
          match external_deps_max.get(dep.peer()) {
            None => external_deps_max.set(dep.peer(), dep.counter())
            Some(old) =>
              if dep.counter() > old {
                external_deps_max.set(dep.peer(), dep.counter())
              }
          }
        }
      }
    }
  }

  let peers: Array[UInt64] = []
  let peer_to_idx: @hashmap.HashMap[UInt64, UInt64] = @hashmap.new(capacity=64)
  let changes: Array[Json] = []

  for blk in decoded_blocks {
    let keys = blk.keys()
    for c in blk.changes() {
      changes.push(change_json_schema(c, keys, peers, peer_to_idx))
    }
  }

  let peers_json: Array[Json] = []
  for p in peers {
    peers_json.push(Json::string(p.to_string()))
  }

  let start_version: Map[String, Json] = Map::new(capacity=external_deps_max.length())
  for pair in external_deps_max.to_array() {
    let (peer, counter) = pair
    start_version[peer.to_string()] = jsonschema_number_int(counter)
  }

  let root: Json = {
    "schema_version": JSON_SCHEMA_VERSION,
    "start_version": Json::object(start_version),
    "peers": Json::array(peers_json),
    "changes": Json::array(changes),
  }

  root.stringify(indent=2)
}
