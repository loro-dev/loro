///|
fn jsonschema_number_i64(i : Int64) -> Json {
  Json::number(i.to_double(), repr=i.to_string())
}

///|
fn jsonschema_number_u64(u : UInt64) -> Json {
  Json::number(u.to_double(), repr=u.to_string())
}

///|
fn jsonschema_number_int(i : Int) -> Json {
  Json::number(i.to_double(), repr=i.to_string())
}
