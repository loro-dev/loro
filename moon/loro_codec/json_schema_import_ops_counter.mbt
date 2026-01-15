///|
fn jsonschema_import_parse_counter_op_content(
  obj : Map[String, Json],
  t : String,
) -> OpContent raise DecodeError {
  // Counter ops are encoded via JsonOpContent::Future (same shape as Unknown),
  // and ultimately stored as raw Value (I64/F64) in ChangeBlock values.
  if t != "counter" && t != "unknown" {
    raise DecodeError(
      "jsonschema_import: invalid Counter op content.type: " + t,
    )
  }
  let prop = jsonschema_import_req_int(obj, "prop", "op.content.prop")
  if prop != 0 {
    raise DecodeError("jsonschema_import: Counter op prop must be 0")
  }
  let value_type = jsonschema_import_req_string(
    obj,
    "value_type",
    "op.content.value_type",
  )
  let value_json = jsonschema_import_req(obj, "value", "op.content.value")
  let raw = match value_type {
    "f64" => {
      let (n, _repr) = jsonschema_import_expect_number(
        value_json,
        "op.content.value",
      )
      Value::F64(n)
    }
    "i64" =>
      Value::I64(
        jsonschema_import_number_to_i64(value_json, "op.content.value"),
      )
    _ =>
      raise DecodeError(
        "jsonschema_import: Counter op unsupported value_type: " + value_type,
      )
  }
  OpContent::Future(FutureOp::Unknown(prop, raw))
}

