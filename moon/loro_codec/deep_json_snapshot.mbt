///|
fn container_id_hex_key(cid : ContainerID) -> String {
  bytes_hex_upper(cid.to_bytes())
}

///|
fn container_id_hex_key_normal(
  peer : UInt64,
  counter : Int,
  kind : ContainerType,
) -> String {
  let w = BytesWriter::new()
  w.write_u8(container_type_to_u8(kind))
  w.write_u64_le(peer)
  w.write_u32_le(counter.reinterpret_as_uint())
  bytes_hex_upper(w.to_bytes())
}

///|
fn common_value_json(
  entries : @hashmap.HashMap[String, (ContainerType, Bytes)],
  v : CommonValue,
  validate : Bool,
  depth : Int,
) -> Json raise DecodeError {
  if depth > 1024 {
    raise DecodeError("snapshot: value too deep")
  }
  match v {
    CommonValue::Null => Json::null()
    CommonValue::Bool(b) => Json::boolean(b)
    CommonValue::Double(d) => Json::number(d)
    CommonValue::I64(x) => Json::number(x.to_double())
    CommonValue::String(s) => Json::string(s)
    CommonValue::Binary(b) => binary_json(b)
    CommonValue::List(items) => {
      let out : Array[Json] = []
      for i in 0..<items.length() {
        out.push(common_value_json(entries, items[i], validate, depth + 1))
      }
      Json::array(out)
    }
    CommonValue::Map(items) => {
      let obj = Map::new(capacity=items.length())
      for pair in items {
        let (k, vv) = pair
        obj[k] = common_value_json(entries, vv, validate, depth + 1)
      }
      Json::object(obj)
    }
    CommonValue::Container(cid) => {
      let key = container_id_hex_key(cid)
      container_json_from_key(entries, key, validate, depth + 1)
    }
  }
}

///|
fn container_json_map(
  entries : @hashmap.HashMap[String, (ContainerType, Bytes)],
  payload : Bytes,
  validate : Bool,
  depth : Int,
) -> Json raise DecodeError {
  let (values, rest1) = postcard_take_map_string_common_value(payload[:])
  let (_deleted_keys, rest2) = postcard_take_vec_string(rest1)
  let (_peers, _meta_bytes) = take_peer_table(rest2)

  let obj = Map::new(capacity=values.length())
  for pair in values {
    let (k, vv) = pair
    obj[k] = common_value_json(entries, vv, validate, depth + 1)
  }
  Json::object(obj)
}

///|
fn container_json_list(
  entries : @hashmap.HashMap[String, (ContainerType, Bytes)],
  payload : Bytes,
  validate : Bool,
  depth : Int,
) -> Json raise DecodeError {
  let (values, _rest1) = postcard_take_vec_common_value(payload[:])
  let out : Array[Json] = []
  for i in 0..<values.length() {
    out.push(common_value_json(entries, values[i], validate, depth + 1))
  }
  Json::array(out)
}

///|
fn container_json_movable_list(
  entries : @hashmap.HashMap[String, (ContainerType, Bytes)],
  payload : Bytes,
  validate : Bool,
  depth : Int,
) -> Json raise DecodeError {
  let (values, _rest1) = postcard_take_vec_common_value(payload[:])
  let out : Array[Json] = []
  for i in 0..<values.length() {
    out.push(common_value_json(entries, values[i], validate, depth + 1))
  }
  Json::array(out)
}

///|
fn container_json_text(payload : Bytes) -> Json raise DecodeError {
  let (text, _rest1) = postcard_take_string(payload[:])
  Json::string(text)
}

///|
fn container_json_counter(payload : Bytes) -> Json raise DecodeError {
  let r = BytesReader::new(payload)
  if r.remaining() != 8 {
    raise DecodeError("counter_state: invalid payload length")
  }
  let bits = r.read_u64_le()
  Json::number(bits.reinterpret_as_double())
}

///|
fn tree_nodes_json(
  entries : @hashmap.HashMap[String, (ContainerType, Bytes)],
  payload : Bytes,
  validate : Bool,
  depth : Int,
) -> Json raise DecodeError {
  if depth > 256 {
    raise DecodeError("tree: too deep")
  }
  let (peers, rest1) = take_peer_table(payload[:])
  let r = BytesReader::from_view(rest1)
  let n_fields = r.read_varint_u64()
  if n_fields != 4UL {
    raise DecodeError("tree: invalid EncodedTree field count")
  }

  // node_ids
  let node_ids_view = r.remaining_view()
  let (node_id_cols, rest_after_node_ids) = take_columnar_vec(node_ids_view)
  r.skip(node_ids_view.length() - rest_after_node_ids.length())
  if node_id_cols.length() != 2 {
    raise DecodeError("tree: invalid node_id column count")
  }
  let node_peer_idx = decode_delta_rle_usize(node_id_cols[0])
  let node_counter = decode_delta_rle_i32(node_id_cols[1])
  if node_peer_idx.length() != node_counter.length() {
    raise DecodeError("tree: node_id column length mismatch")
  }

  // nodes
  let nodes_view = r.remaining_view()
  let (node_cols, rest_after_nodes) = take_columnar_vec(nodes_view)
  r.skip(nodes_view.length() - rest_after_nodes.length())
  if node_cols.length() != 5 {
    raise DecodeError("tree: invalid node column count")
  }
  let parent_idx_plus_two = decode_delta_rle_usize(node_cols[0])
  let last_set_peer_idx = decode_delta_rle_usize(node_cols[1])
  let last_set_counter = decode_delta_rle_i32(node_cols[2])
  let last_set_lamport_sub = decode_delta_rle_i32(node_cols[3])
  let fractional_idx_idx = decode_postcard_vec_usize(node_cols[4])
  let n_nodes = node_peer_idx.length()
  if parent_idx_plus_two.length() != n_nodes ||
    last_set_peer_idx.length() != n_nodes ||
    last_set_counter.length() != n_nodes ||
    last_set_lamport_sub.length() != n_nodes ||
    fractional_idx_idx.length() != n_nodes {
    raise DecodeError("tree: node column length mismatch")
  }

  let frac_view = r.remaining_view()
  let (fractional_indexes_bytes, rest_after_frac) = postcard_take_bytes(
    frac_view,
  )
  r.skip(frac_view.length() - rest_after_frac.length())
  let reserved_view = r.remaining_view()
  let (_reserved_bytes, rest_after_reserved) = postcard_take_bytes(reserved_view)
  r.skip(reserved_view.length() - rest_after_reserved.length())
  if r.remaining() != 0 {
    raise DecodeError("tree: trailing bytes")
  }
  let positions = decode_position_arena_v2(fractional_indexes_bytes[:])

  // Alive nodes are encoded first. Deleted nodes follow (and the first deleted node has parent=Deleted).
  let mut alive_n = n_nodes
  for i in 0..<n_nodes {
    if parent_idx_plus_two[i] == 1UL {
      alive_n = i
      break
    }
  }

  // Per-node fields (for alive nodes only).
  let id_strs : Array[String] = []
  let parent_idx : Array[Int?] = []
  let fi_hex : Array[String] = []
  let meta_key : Array[String] = []

  for i in 0..<alive_n {
    let peer_idx_u64 = node_peer_idx[i]
    if validate {
      if peer_idx_u64 > 0x7FFF_FFFFUL ||
        peer_idx_u64.to_int() < 0 ||
        peer_idx_u64.to_int() >= peers.length() {
        raise DecodeError("tree: node peer_idx out of range")
      }
    }
    let peer = peers[peer_idx_u64.to_int()]
    let counter = node_counter[i]
    id_strs.push(counter.to_string() + "@" + peer.to_string())
    let p = parent_idx_plus_two[i]
    if p == 0UL {
      parent_idx.push(None)
    } else if p >= 2UL {
      let idx = (p - 2UL).to_int()
      if validate && (idx < 0 || idx >= alive_n) {
        raise DecodeError("tree: invalid parent index")
      }
      parent_idx.push(Some(idx))
    } else {
      // parent=Deleted should not appear for alive nodes.
      raise DecodeError("tree: unexpected deleted parent in alive nodes")
    }
    let fi_idx = fractional_idx_idx[i].to_int()
    if validate && (fi_idx < 0 || fi_idx >= positions.length()) {
      raise DecodeError("tree: invalid fractional_index_idx")
    }
    fi_hex.push(bytes_hex_upper(positions[fi_idx]))
    meta_key.push(container_id_hex_key_normal(peer, counter, ContainerType::map()))
  }

  // children lists
  let children : Array[Array[Int]] = []
  for _i in 0..<alive_n {
    children.push([])
  }
  let roots : Array[Int] = []
  for i in 0..<alive_n {
    match parent_idx[i] {
      None => roots.push(i)
      Some(p) => children[p].push(i)
    }
  }

  // sort siblings by fractional_index, and assign index field
  let index_arr : Array[Int] = []
  for _i in 0..<alive_n {
    index_arr.push(0)
  }

  roots.sort_by_key(i => fi_hex[i])
  for i in 0..<roots.length() {
    index_arr[roots[i]] = i
  }
  for p in 0..<alive_n {
    children[p].sort_by_key(i => fi_hex[i])
    for i in 0..<children[p].length() {
      index_arr[children[p][i]] = i
    }
  }

  fn node_json(
    entries : @hashmap.HashMap[String, (ContainerType, Bytes)],
    id_strs : Array[String],
    parent_idx : Array[Int?],
    fi_hex : Array[String],
    meta_key : Array[String],
    index_arr : Array[Int],
    children : Array[Array[Int]],
    i : Int,
    validate : Bool,
    depth : Int,
  ) -> Json raise DecodeError {
    let obj = Map::new(capacity=6)
    obj["parent"] = match parent_idx[i] {
      None => Json::null()
      Some(p) => Json::string(id_strs[p])
    }
    obj["meta"] = container_json_from_key(
      entries,
      meta_key[i],
      validate,
      depth + 1,
    )
    obj["id"] = Json::string(id_strs[i])
    obj["index"] = Json::number(index_arr[i].to_double())
    let out_children : Array[Json] = []
    for c in children[i] {
      out_children.push(
        node_json(
          entries,
          id_strs,
          parent_idx,
          fi_hex,
          meta_key,
          index_arr,
          children,
          c,
          validate,
          depth + 1,
        ),
      )
    }
    obj["children"] = Json::array(out_children)
    obj["fractional_index"] = Json::string(fi_hex[i])
    Json::object(obj)
  }

  let out_roots : Array[Json] = []
  for i in roots {
    out_roots.push(
      node_json(
        entries,
        id_strs,
        parent_idx,
        fi_hex,
        meta_key,
        index_arr,
        children,
        i,
        validate,
        depth + 1,
      ),
    )
  }
  Json::array(out_roots)
}

///|
fn container_json_from_key(
  entries : @hashmap.HashMap[String, (ContainerType, Bytes)],
  key : String,
  validate : Bool,
  depth : Int,
) -> Json raise DecodeError {
  if depth > 2048 {
    raise DecodeError("snapshot: container nesting too deep")
  }
  match entries.get(key) {
    None => raise DecodeError("snapshot: missing container state")
    Some((kind, payload)) =>
      match kind {
        ContainerType::Map => container_json_map(entries, payload, validate, depth)
        ContainerType::List =>
          container_json_list(entries, payload, validate, depth)
        ContainerType::Text => container_json_text(payload)
        ContainerType::Tree => tree_nodes_json(entries, payload, validate, depth)
        ContainerType::MovableList =>
          container_json_movable_list(entries, payload, validate, depth)
        ContainerType::Counter => container_json_counter(payload)
        ContainerType::Unknown(_) =>
          raise DecodeError("snapshot: unsupported container type")
      }
  }
}

///|
fn deep_json_from_state_kv_store(
  state_bytes : Bytes,
  validate : Bool,
) -> Json raise DecodeError {
  // For shallow_root_state_bytes in non-shallow snapshots.
  if state_bytes.length() == 0 {
    return Json::object(Map::new(capacity=0))
  }
  // Special sentinel for empty state.
  if state_bytes.length() == 1 && state_bytes[0] == b'E' {
    return Json::object(Map::new(capacity=0))
  }

  let kvs = sstable_import_all(state_bytes, validate)
  let entries : @hashmap.HashMap[String, (ContainerType, Bytes)] = @hashmap.new(
    capacity=if kvs.length() < 16 { 16 } else { kvs.length() },
  )
  let roots : Array[(String, String)] = []
  for kv in kvs {
    let (k, v) = kv
    if k.length() == 2 && k[0] == b'f' && k[1] == b'r' {
      continue
    }
    let cid = container_id_from_bytes(k[:])
    let key = bytes_hex_upper(k)
    let wrapper = parse_container_wrapper(v[:])
    entries.set(key, (wrapper.kind(), wrapper.payload_view().to_bytes()))
    match cid {
      ContainerID::Root(name, _kind) => roots.push((name, key))
      ContainerID::Normal(_, _, _) => ()
    }
  }

  let obj = Map::new(capacity=roots.length())
  for pair in roots {
    let (name, key) = pair
    obj[name] = container_json_from_key(entries, key, validate, 0)
  }
  Json::object(obj)
}

///|\
/// Export a FastSnapshot (mode=3) document blob into deep JSON format,
/// matching Rust `doc.get_deep_value().to_json_value()`.
pub fn export_deep_json_from_fast_snapshot(
  bytes : Bytes,
  validate : Bool,
) -> String raise DecodeError {
  let doc = parse_document(bytes, validate)
  if doc.mode() != 3 {
    raise DecodeError("deep-json: not a FastSnapshot (mode=3) document")
  }
  let parsed = parse_fast_snapshot_body(doc.body_view())
  let root = deep_json_from_state_kv_store(
    parsed.state_bytes_view().to_bytes(),
    validate,
  )
  root.stringify(indent=2)
}
