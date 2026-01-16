///|
fn jsonschema_import_hex_nibble(c : UInt16) -> Int raise DecodeError {
  let x = c.to_int()
  if x >= '0'.to_int() && x <= '9'.to_int() {
    x - '0'.to_int()
  } else if x >= 'a'.to_int() && x <= 'f'.to_int() {
    10 + (x - 'a'.to_int())
  } else if x >= 'A'.to_int() && x <= 'F'.to_int() {
    10 + (x - 'A'.to_int())
  } else {
    raise DecodeError("jsonschema_import: invalid hex char")
  }
}

///|
fn jsonschema_import_hex_to_bytes(hex : String) -> Bytes raise DecodeError {
  if hex.length() % 2 != 0 {
    raise DecodeError("jsonschema_import: invalid hex length")
  }
  let out_len = hex.length() / 2
  Bytes::makei(out_len, i => {
    let hi = jsonschema_import_hex_nibble(hex[2 * i])
    let lo = jsonschema_import_hex_nibble(hex[2 * i + 1])
    ((hi << 4) | lo).to_byte()
  })
}
