///|
const JSON_SCHEMA_VERSION : Int = 1

///|
fn change_json_schema(
  change : Change,
  keys : Array[String],
  peers : Array[UInt64],
  peer_to_idx : @hashmap.HashMap[UInt64, UInt64],
) -> Json raise DecodeError {
  let deps : Array[String] = []
  for d in change.deps() {
    deps.push(id_string_with_peer_index(d, peers, peer_to_idx))
  }
  let ops : Array[Json] = []
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

///|
/// Export a FastUpdates (mode=4) document blob into JSON schema format as described in
/// `docs/JsonSchema.md` (peer-compressed).
pub fn export_json_schema_from_fast_updates(
  bytes : Bytes,
  validate : Bool,
) -> String raise DecodeError {
  let doc = parse_document(bytes, validate)
  if doc.mode() != 4 {
    raise DecodeError("jsonschema: not a FastUpdates (mode=4) document")
  }

  // Decode blocks first so we can detect which deps are outside the exported set.
  let blocks = parse_fast_updates_body(doc.body_view())
  let decoded_blocks : Array[DecodedChangeBlock] = []
  for b in blocks {
    decoded_blocks.push(decode_change_block_full(b))
  }

  // Determine the per-peer exported starting counter (like start_vv[peer]).
  // FastUpdates always include a contiguous counter range per peer, so deps are external iff
  // `dep.counter < start_counter` or the peer isn't exported at all.
  let start_counter_by_peer : @hashmap.HashMap[UInt64, Int] = @hashmap.new(
    capacity=64,
  )
  for blk in decoded_blocks {
    for c in blk.changes() {
      let id = c.id()
      let peer = id.peer()
      let counter = id.counter()
      match start_counter_by_peer.get(peer) {
        None => start_counter_by_peer.set(peer, counter)
        Some(old) =>
          if counter < old {
            start_counter_by_peer.set(peer, counter)
          }
      }
    }
  }

  // Derive `start_version` by collecting deps that are outside the exported set.
  let external_deps_max : @hashmap.HashMap[UInt64, Int] = @hashmap.new(
    capacity=64,
  )
  for blk in decoded_blocks {
    for c in blk.changes() {
      for dep in c.deps() {
        let is_external = match start_counter_by_peer.get(dep.peer()) {
          None => true
          Some(start_counter) => dep.counter() < start_counter
        }
        if is_external {
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
  let peers : Array[UInt64] = []
  let peer_to_idx : @hashmap.HashMap[UInt64, UInt64] = @hashmap.new(capacity=64)
  let changes : Array[Json] = []
  for blk in decoded_blocks {
    let keys = blk.keys()
    for c in blk.changes() {
      changes.push(change_json_schema(c, keys, peers, peer_to_idx))
    }
  }
  let peers_json : Array[Json] = []
  for p in peers {
    peers_json.push(Json::string(p.to_string()))
  }
  let start_version : Map[String, Json] = Map::new(
    capacity=external_deps_max.length(),
  )
  for pair in external_deps_max.to_array() {
    let (peer, counter) = pair
    start_version[peer.to_string()] = jsonschema_number_int(counter)
  }
  let root : Json = {
    "schema_version": JSON_SCHEMA_VERSION,
    "start_version": Json::object(start_version),
    "peers": Json::array(peers_json),
    "changes": Json::array(changes),
  }
  root.stringify(indent=2)
}
