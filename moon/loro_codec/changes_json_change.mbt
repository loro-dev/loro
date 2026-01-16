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
