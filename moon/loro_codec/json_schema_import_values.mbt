///|
fn jsonschema_import_loro_value(
  v : Json,
  op_id : ID,
  keys : Array[String],
  key_to_idx : @hashmap.HashMap[String, UInt64],
  peers : Array[UInt64]?,
) -> LoroValue raise DecodeError {
  match v {
    Json::Null => LoroValue::Null
    Json::True => LoroValue::True
    Json::False => LoroValue::False
    Json::Number(_n, repr~) =>
      // Decide int vs float by lexical repr when available.
      match repr {
        Some(s) =>
          if s.contains(".") || s.contains("e") || s.contains("E") {
            LoroValue::F64(_n)
          } else {
            LoroValue::I64(
              jsonschema_import_parse_int64_decimal(s, "loro_value.i64"),
            )
          }
        None => LoroValue::F64(_n)
      }
    Json::String(s) =>
      if s.has_prefix(LORO_CONTAINER_ID_PREFIX) {
        let cid_str = (s[LORO_CONTAINER_ID_PREFIX.length():] catch {
          _ =>
            raise DecodeError(
              "jsonschema_import: invalid container value string",
            )
        }).to_string()
        let cid = jsonschema_import_parse_container_id(cid_str, peers)
        match cid {
          ContainerID::Root(_, _kind) =>
            // Root container ids are not representable in binary value encoding; reject for now.
            raise DecodeError(
              "jsonschema_import: root container value is not supported",
            )
          ContainerID::Normal(peer, counter, kind) => {
            // Binary value encoding reconstructs container id from op_id + type.
            if peer != op_id.peer() || counter != op_id.counter() {
              raise DecodeError(
                "jsonschema_import: container value id mismatch",
              )
            }
            LoroValue::ContainerType(container_type_to_u8(kind))
          }
        }
      } else {
        LoroValue::Str(s)
      }
    Json::Array(arr) => {
      let out : Array[LoroValue] = []
      for i in 0..<arr.length() {
        out.push(
          jsonschema_import_loro_value(
            arr[i],
            id_inc(op_id, i),
            keys,
            key_to_idx,
            peers,
          ),
        )
      }
      LoroValue::List(out)
    }
    Json::Object(obj) => {
      let out : Array[(UInt64, LoroValue)] = []
      for k, vv in obj {
        let idx = register_key(keys, key_to_idx, k)
        out.push(
          (
            idx,
            jsonschema_import_loro_value(vv, op_id, keys, key_to_idx, peers),
          ),
        )
      }
      LoroValue::Map(out)
    }
  }
}
