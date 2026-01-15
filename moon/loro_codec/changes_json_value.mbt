///|
fn loro_value_json(v : LoroValue, keys : Array[String], depth : Int) -> Json {
  if depth > 1024 {
    return Json::string("<too deep>")
  }
  match v {
    LoroValue::Null => Json::null()
    LoroValue::True => Json::boolean(true)
    LoroValue::False => Json::boolean(false)
    LoroValue::I64(x) => Json::string(x.to_string())
    LoroValue::F64(x) => Json::number(x)
    LoroValue::Str(s) => Json::string(s)
    LoroValue::Binary(b) => binary_json(b)
    LoroValue::List(items) => {
      let out : Array[Json] = []
      for it in items {
        out.push(loro_value_json(it, keys, depth + 1))
      }
      Json::array(out)
    }
    LoroValue::Map(items) => {
      let obj = Map::new(capacity=items.length())
      for pair in items {
        let (key_idx, it) = pair
        let mut key = "<key_idx:" + key_idx.to_string() + ">"
        if key_idx <= 0x7FFF_FFFFUL {
          let i = key_idx.to_int()
          if i >= 0 && i < keys.length() {
            key = keys[i]
          }
        }
        obj[key] = loro_value_json(it, keys, depth + 1)
      }
      Json::object(obj)
    }
    LoroValue::ContainerType(ct) => Json::number(ct.to_uint().to_double())
  }
}
