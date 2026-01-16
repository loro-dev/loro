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
