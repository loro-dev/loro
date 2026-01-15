///|
fn jsonschema_register_peer(
  peers : Array[UInt64],
  peer_to_idx : @hashmap.HashMap[UInt64, UInt64],
  peer : UInt64,
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

///|
fn id_string_with_peer_index(
  id : ID,
  peers : Array[UInt64],
  peer_to_idx : @hashmap.HashMap[UInt64, UInt64],
) -> String {
  let idx = jsonschema_register_peer(peers, peer_to_idx, id.peer())
  id.counter().to_string() + "@" + idx.to_string()
}

///|
fn idlp_string_with_peer_index(
  idlp : IdLp,
  peers : Array[UInt64],
  peer_to_idx : @hashmap.HashMap[UInt64, UInt64],
) -> String {
  let idx = jsonschema_register_peer(peers, peer_to_idx, idlp.peer())
  "L" + idlp.lamport().to_uint64().to_string() + "@" + idx.to_string()
}

///|
fn jsonschema_container_type_string(kind : ContainerType) -> String {
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
fn container_id_string_with_peer_index(
  cid : ContainerID,
  peers : Array[UInt64],
  peer_to_idx : @hashmap.HashMap[UInt64, UInt64],
) -> String {
  match cid {
    ContainerID::Root(name, kind) =>
      "cid:root-" + name + ":" + jsonschema_container_type_string(kind)
    ContainerID::Normal(peer, counter, kind) => {
      let idx = jsonschema_register_peer(peers, peer_to_idx, peer)
      "cid:" +
      counter.to_string() +
      "@" +
      idx.to_string() +
      ":" +
      jsonschema_container_type_string(kind)
    }
  }
}
