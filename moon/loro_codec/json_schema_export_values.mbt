///|
fn loro_value_json_schema(
  v : LoroValue,
  keys : Array[String],
  id : ID,
  peers : Array[UInt64],
  peer_to_idx : @hashmap.HashMap[UInt64, UInt64],
  depth : Int,
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
      let out : Array[Json] = []
      for i in 0..<items.length() {
        let child_id = id_inc(id, i)
        out.push(
          loro_value_json_schema(
            items[i],
            keys,
            child_id,
            peers,
            peer_to_idx,
            depth + 1,
          ),
        )
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
        obj[key] = loro_value_json_schema(
          value,
          keys,
          id,
          peers,
          peer_to_idx,
          depth + 1,
        )
      }
      Json::object(obj)
    }
    LoroValue::ContainerType(ct) => {
      let kind = container_type_from_u8(ct)
      let peer_idx = jsonschema_register_peer(peers, peer_to_idx, id.peer())
      let cid = "cid:" +
        id.counter().to_string() +
        "@" +
        peer_idx.to_string() +
        ":" +
        jsonschema_container_type_string(kind)
      Json::string(LORO_CONTAINER_ID_PREFIX + cid)
    }
  }
}
