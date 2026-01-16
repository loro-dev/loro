///|
const LORO_CONTAINER_ID_PREFIX : String = "🦜:"

///|
fn id_string(id : @loro_codec.ID) -> String {
  id.counter().to_string() + "@" + id.peer().to_string()
}

///|
fn container_type_string(kind : @loro_codec.ContainerType) -> String {
  match kind {
    @loro_codec.ContainerType::Map => "Map"
    @loro_codec.ContainerType::List => "List"
    @loro_codec.ContainerType::Text => "Text"
    @loro_codec.ContainerType::Tree => "Tree"
    @loro_codec.ContainerType::MovableList => "MovableList"
    @loro_codec.ContainerType::Counter => "Counter"
    @loro_codec.ContainerType::Unknown(k) => "Unknown(" + k.to_string() + ")"
  }
}

///|
fn container_id_string(cid : @loro_codec.ContainerID) -> String {
  match cid {
    @loro_codec.ContainerID::Root(name, kind) =>
      "cid:root-" + name + ":" + container_type_string(kind)
    @loro_codec.ContainerID::Normal(peer, counter, kind) =>
      "cid:" +
      counter.to_string() +
      "@" +
      peer.to_string() +
      ":" +
      container_type_string(kind)
  }
}

///|
fn check_root_container_name(name : String) -> Unit raise LoroError {
  if name.length() == 0 {
    raise LoroError("invalid root container name: empty")
  }
  if name.contains("/") {
    raise LoroError("invalid root container name: contains '/'")
  }
  if name.contains("\u0000") {
    raise LoroError("invalid root container name: contains '\\0'")
  }
}

///|
fn idlp_compare(a : @loro_codec.IdLp, b : @loro_codec.IdLp) -> Int {
  let la = a.lamport()
  let lb = b.lamport()
  if la < lb {
    return -1
  }
  if la > lb {
    return 1
  }
  let pa = a.peer()
  let pb = b.peer()
  if pa < pb {
    return -1
  }
  if pa > pb {
    return 1
  }
  0
}

///|
fn idlp_gt(a : @loro_codec.IdLp, b : @loro_codec.IdLp) -> Bool {
  idlp_compare(a, b) > 0
}

///|
fn change_atom_len(change : @loro_codec.Change) -> Int {
  let mut sum : UInt64 = 0
  for op in change.ops() {
    sum = sum + op.len().to_uint64()
  }
  sum.to_int()
}

///|
fn change_last_id(change : @loro_codec.Change) -> @loro_codec.ID {
  let len = change_atom_len(change)
  @loro_codec.ID::new(change.id().peer(), change.id().counter() + len - 1)
}

///|
fn change_last_lamport(change : @loro_codec.Change) -> UInt {
  let len = change_atom_len(change)
  change.lamport() + (len - 1).reinterpret_as_uint()
}

///|
fn vv_to_map(vv : @loro_codec.VersionVector) -> @hashmap.HashMap[UInt64, Int] {
  let m = @hashmap.new(capacity=vv.length())
  for pair in vv {
    let (peer, next_counter) = pair
    m.set(peer, next_counter)
  }
  m
}

///|
fn vv_from_map(vv : @hashmap.HashMap[UInt64, Int]) -> @loro_codec.VersionVector {
  let out : @loro_codec.VersionVector = []
  for pair in vv {
    let (peer, next_counter) = pair
    out.push((peer, next_counter))
  }
  out.sort_by_key(pair => {
    let (peer, _next_counter) = pair
    peer
  })
  out
}

///|
fn vv_get(vv : @hashmap.HashMap[UInt64, Int], peer : UInt64) -> Int {
  match vv.get(peer) {
    None => 0
    Some(v) => v
  }
}

///|
fn vv_set_max(
  vv : @hashmap.HashMap[UInt64, Int],
  peer : UInt64,
  next_counter : Int,
) -> Unit {
  let current = vv_get(vv, peer)
  if next_counter > current {
    vv.set(peer, next_counter)
  }
}

///|
fn register_key(
  keys : Array[String],
  key_to_idx : @hashmap.HashMap[String, UInt64],
  key : String,
) -> UInt64 {
  match key_to_idx.get(key) {
    Some(idx) => idx
    None => {
      let idx = keys.length().to_uint64()
      keys.push(key)
      key_to_idx.set(key, idx)
      idx
    }
  }
}
