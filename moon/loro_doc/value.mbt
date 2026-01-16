///|
pub enum Value {
  Null
  True
  False
  I64(Int64)
  F64(Double)
  Str(String)
  Binary(Bytes)
  List(Array[Value])
  Map(Array[(String, Value)])
  Container(@loro_codec.ContainerID)
} derive(Eq, Show)

///|
fn binary_json(bytes : Bytes) -> Json {
  let out : Array[Json] = []
  for b in bytes {
    out.push(Json::number(b.to_uint().to_double()))
  }
  Json::array(out)
}

///|
fn value_to_json_shallow(value : Value) -> Json {
  match value {
    Value::Null => Json::null()
    Value::True => Json::boolean(true)
    Value::False => Json::boolean(false)
    Value::I64(i) => Json::number(i.to_double(), repr=i.to_string())
    Value::F64(f) => Json::number(f)
    Value::Str(s) => Json::string(s)
    Value::Binary(b) => binary_json(b)
    Value::List(items) => {
      let out : Array[Json] = []
      for it in items {
        out.push(value_to_json_shallow(it))
      }
      Json::array(out)
    }
    Value::Map(items) => {
      let obj = Map::new(capacity=items.length())
      for pair in items {
        let (k, v) = pair
        obj[k] = value_to_json_shallow(v)
      }
      Json::object(obj)
    }
    Value::Container(cid) =>
      Json::string(LORO_CONTAINER_ID_PREFIX + container_id_string(cid))
  }
}

///|
fn id_inc(id : @loro_codec.ID, delta : Int) -> @loro_codec.ID raise LoroError {
  let mut counter64 = id.counter().to_int64()
  counter64 = counter64 + delta.to_int64()
  if counter64 < -2147483648L || counter64 > 2147483647L {
    raise LoroError("id counter overflow")
  }
  @loro_codec.ID::new(id.peer(), counter64.to_int())
}

///|
fn encode_value_to_loro_value(
  value : Value,
  keys : Array[String],
  key_to_idx : @hashmap.HashMap[String, UInt64],
) -> @loro_codec.LoroValue raise LoroError {
  match value {
    Value::Null => @loro_codec.LoroValue::Null
    Value::True => @loro_codec.LoroValue::True
    Value::False => @loro_codec.LoroValue::False
    Value::I64(i) => @loro_codec.LoroValue::I64(i)
    Value::F64(f) => @loro_codec.LoroValue::F64(f)
    Value::Str(s) => @loro_codec.LoroValue::Str(s)
    Value::Binary(b) => @loro_codec.LoroValue::Binary(b)
    Value::List(items) => {
      let out : Array[@loro_codec.LoroValue] = []
      for it in items {
        out.push(encode_value_to_loro_value(it, keys, key_to_idx))
      }
      @loro_codec.LoroValue::List(out)
    }
    Value::Map(items) => {
      let out : Array[(UInt64, @loro_codec.LoroValue)] = []
      for pair in items {
        let (k, v) = pair
        let idx = register_key(keys, key_to_idx, k)
        out.push((idx, encode_value_to_loro_value(v, keys, key_to_idx)))
      }
      @loro_codec.LoroValue::Map(out)
    }
    Value::Container(_cid) =>
      raise LoroError("value encoding: Container is not supported here")
  }
}

///|
fn value_contains_container(value : Value) -> Bool {
  match value {
    Value::Container(_) => true
    Value::List(items) => {
      for it in items {
        if value_contains_container(it) {
          return true
        }
      }
      false
    }
    Value::Map(items) => {
      for pair in items {
        let (_k, v) = pair
        if value_contains_container(v) {
          return true
        }
      }
      false
    }
    _ => false
  }
}

///|
fn normalize_loro_value_keys(
  value : @loro_codec.LoroValue,
  block_keys : Array[String],
  doc_keys : Array[String],
  doc_key_to_idx : @hashmap.HashMap[String, UInt64],
) -> @loro_codec.LoroValue raise LoroError {
  match value {
    @loro_codec.LoroValue::Map(items) => {
      let out : Array[(UInt64, @loro_codec.LoroValue)] = []
      for pair in items {
        let (k_idx, v) = pair
        if k_idx > 0x7FFF_FFFFUL {
          raise LoroError("loro_value: key_idx too large")
        }
        let i = k_idx.to_int()
        if i < 0 || i >= block_keys.length() {
          raise LoroError("loro_value: key_idx out of range")
        }
        let key = block_keys[i]
        let new_idx = register_key(doc_keys, doc_key_to_idx, key)
        out.push(
          (
            new_idx,
            normalize_loro_value_keys(v, block_keys, doc_keys, doc_key_to_idx),
          ),
        )
      }
      @loro_codec.LoroValue::Map(out)
    }
    @loro_codec.LoroValue::List(items) => {
      let out : Array[@loro_codec.LoroValue] = []
      for it in items {
        out.push(
          normalize_loro_value_keys(it, block_keys, doc_keys, doc_key_to_idx),
        )
      }
      @loro_codec.LoroValue::List(out)
    }
    _ => value
  }
}

///|
fn decode_value_from_loro_value(
  value : @loro_codec.LoroValue,
  keys : Array[String],
  op_id : @loro_codec.ID,
) -> Value raise LoroError {
  match value {
    @loro_codec.LoroValue::Null => Value::Null
    @loro_codec.LoroValue::True => Value::True
    @loro_codec.LoroValue::False => Value::False
    @loro_codec.LoroValue::I64(i) => Value::I64(i)
    @loro_codec.LoroValue::F64(f) => Value::F64(f)
    @loro_codec.LoroValue::Str(s) => Value::Str(s)
    @loro_codec.LoroValue::Binary(b) => Value::Binary(b)
    @loro_codec.LoroValue::List(items) => {
      let out : Array[Value] = []
      for i in 0..<items.length() {
        out.push(
          decode_value_from_loro_value(items[i], keys, id_inc(op_id, i)),
        )
      }
      Value::List(out)
    }
    @loro_codec.LoroValue::Map(items) => {
      let out : Array[(String, Value)] = []
      for pair in items {
        let (k_idx, v) = pair
        if k_idx > 0x7FFF_FFFFUL {
          raise LoroError("loro_value: key_idx too large")
        }
        let i = k_idx.to_int()
        if i < 0 || i >= keys.length() {
          raise LoroError("loro_value: key_idx out of range")
        }
        out.push(
          (keys[i], decode_value_from_loro_value(v, keys, op_id)),
        )
      }
      Value::Map(out)
    }
    @loro_codec.LoroValue::ContainerType(ct) => {
      let kind = @loro_codec.container_type_from_u8(ct)
      let cid = @loro_codec.ContainerID::normal(op_id.peer(), op_id.counter(), kind)
      Value::Container(cid)
    }
  }
}
