///|
fn jsonschema_import_parse_uint64_decimal(
  s : String,
  what : String,
) -> UInt64 raise DecodeError {
  @strconv.parse_uint64(s[:]) catch {
    @strconv.StrConvError(err) =>
      raise DecodeError("jsonschema_import: invalid " + what + ": " + err)
  }
}

///|
fn jsonschema_import_parse_int64_decimal(
  s : String,
  what : String,
) -> Int64 raise DecodeError {
  @strconv.parse_int64(s[:]) catch {
    @strconv.StrConvError(err) =>
      raise DecodeError("jsonschema_import: invalid " + what + ": " + err)
  }
}

///|
fn jsonschema_import_parse_int_decimal(
  s : String,
  what : String,
) -> Int raise DecodeError {
  @strconv.parse_int(s[:]) catch {
    @strconv.StrConvError(err) =>
      raise DecodeError("jsonschema_import: invalid " + what + ": " + err)
  }
}

///|
fn jsonschema_import_expect_object(
  v : Json,
  what : String,
) -> Map[String, Json] raise DecodeError {
  match v {
    Json::Object(obj) => obj
    _ => raise DecodeError("jsonschema_import: expected object for " + what)
  }
}

///|
fn jsonschema_import_expect_array(
  v : Json,
  what : String,
) -> Array[Json] raise DecodeError {
  match v {
    Json::Array(arr) => arr
    _ => raise DecodeError("jsonschema_import: expected array for " + what)
  }
}

///|
fn jsonschema_import_expect_string(
  v : Json,
  what : String,
) -> String raise DecodeError {
  match v {
    Json::String(s) => s
    _ => raise DecodeError("jsonschema_import: expected string for " + what)
  }
}

///|
fn jsonschema_import_expect_number(
  v : Json,
  what : String,
) -> (Double, String?) raise DecodeError {
  match v {
    Json::Number(n, repr~) => (n, repr)
    _ => raise DecodeError("jsonschema_import: expected number for " + what)
  }
}

///|
fn jsonschema_import_req(
  obj : Map[String, Json],
  key : String,
  what : String,
) -> Json raise DecodeError {
  match obj.get(key) {
    Some(v) => v
    None => raise DecodeError("jsonschema_import: missing " + what)
  }
}

///|
fn jsonschema_import_req_string(
  obj : Map[String, Json],
  key : String,
  what : String,
) -> String raise DecodeError {
  jsonschema_import_expect_string(jsonschema_import_req(obj, key, what), what)
}

///|
fn jsonschema_import_req_array(
  obj : Map[String, Json],
  key : String,
  what : String,
) -> Array[Json] raise DecodeError {
  jsonschema_import_expect_array(jsonschema_import_req(obj, key, what), what)
}

///|
fn jsonschema_import_req_int(
  obj : Map[String, Json],
  key : String,
  what : String,
) -> Int raise DecodeError {
  jsonschema_import_number_to_int(jsonschema_import_req(obj, key, what), what)
}

///|
fn jsonschema_import_req_i64(
  obj : Map[String, Json],
  key : String,
  what : String,
) -> Int64 raise DecodeError {
  jsonschema_import_number_to_i64(jsonschema_import_req(obj, key, what), what)
}

///|
fn jsonschema_import_req_u32(
  obj : Map[String, Json],
  key : String,
  what : String,
) -> UInt raise DecodeError {
  jsonschema_import_number_to_u32(jsonschema_import_req(obj, key, what), what)
}

///|
fn jsonschema_import_number_to_int(
  v : Json,
  what : String,
) -> Int raise DecodeError {
  let (n, repr) = jsonschema_import_expect_number(v, what)
  // Prefer repr for integer decoding to avoid float rounding.
  match repr {
    Some(s) => jsonschema_import_parse_int_decimal(s, what)
    None => {
      // Best-effort: require it to be an integer.
      let i = n.to_int()
      if i.to_double() != n {
        raise DecodeError(
          "jsonschema_import: expected integer number for " + what,
        )
      }
      i
    }
  }
}

///|
fn jsonschema_import_number_to_i64(
  v : Json,
  what : String,
) -> Int64 raise DecodeError {
  let (n, repr) = jsonschema_import_expect_number(v, what)
  match repr {
    Some(s) => jsonschema_import_parse_int64_decimal(s, what)
    None => {
      let i = n.to_int64()
      if i.to_double() != n {
        raise DecodeError(
          "jsonschema_import: expected integer number for " + what,
        )
      }
      i
    }
  }
}

///|
fn jsonschema_import_number_to_u32(
  v : Json,
  what : String,
) -> UInt raise DecodeError {
  let i = jsonschema_import_number_to_int(v, what)
  if i < 0 {
    raise DecodeError(
      "jsonschema_import: expected non-negative integer for " + what,
    )
  }
  i.reinterpret_as_uint()
}
