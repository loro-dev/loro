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
