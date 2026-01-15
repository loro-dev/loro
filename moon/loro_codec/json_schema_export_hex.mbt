///|
fn jsonschema_bytes_hex_upper(bytes : Bytes) -> String {
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
fn jsonschema_binary_json(bytes : Bytes) -> Json {
  let out : Array[Json] = []
  for b in bytes {
    out.push(Json::number(b.to_uint().to_double()))
  }
  Json::array(out)
}
