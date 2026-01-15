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
/// This is intended for tooling/tests (JsonSchema <-> binary). It only supports
/// the subset of JsonSchema that corresponds to current containers (Map/List/Text/Tree/MovableList)
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
