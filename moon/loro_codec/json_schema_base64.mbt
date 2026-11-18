///|
fn jsonschema_bytes_to_base64(bytes : Bytes) -> String {
  let alphabet : Array[Char] = [
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P',
    'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'a', 'b', 'c', 'd', 'e', 'f',
    'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v',
    'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '+', '/',
  ]
  let sb = StringBuilder::new()
  let len = bytes.length()
  let full_end = len - len % 3
  let mut i = 0
  while i < full_end {
    let b0 = bytes[i].to_uint()
    let b1 = bytes[i + 1].to_uint()
    let b2 = bytes[i + 2].to_uint()
    let n = (b0 << 16) | (b1 << 8) | b2
    sb.write_char(alphabet[((n >> 18) & 0x3FU).reinterpret_as_int()])
    sb.write_char(alphabet[((n >> 12) & 0x3FU).reinterpret_as_int()])
    sb.write_char(alphabet[((n >> 6) & 0x3FU).reinterpret_as_int()])
    sb.write_char(alphabet[(n & 0x3FU).reinterpret_as_int()])
    i = i + 3
  }
  let rem = len - full_end
  if rem == 1 {
    let b0 = bytes[full_end].to_uint()
    let n = b0 << 16
    sb.write_char(alphabet[((n >> 18) & 0x3FU).reinterpret_as_int()])
    sb.write_char(alphabet[((n >> 12) & 0x3FU).reinterpret_as_int()])
    sb.write_char('=')
    sb.write_char('=')
  } else if rem == 2 {
    let b0 = bytes[full_end].to_uint()
    let b1 = bytes[full_end + 1].to_uint()
    let n = (b0 << 16) | (b1 << 8)
    sb.write_char(alphabet[((n >> 18) & 0x3FU).reinterpret_as_int()])
    sb.write_char(alphabet[((n >> 12) & 0x3FU).reinterpret_as_int()])
    sb.write_char(alphabet[((n >> 6) & 0x3FU).reinterpret_as_int()])
    sb.write_char('=')
  }
  sb.to_string()
}

///|
fn jsonschema_import_base64_sextet(c : UInt16) -> Int raise DecodeError {
  let x = c.to_int()
  if x >= 'A'.to_int() && x <= 'Z'.to_int() {
    x - 'A'.to_int()
  } else if x >= 'a'.to_int() && x <= 'z'.to_int() {
    26 + (x - 'a'.to_int())
  } else if x >= '0'.to_int() && x <= '9'.to_int() {
    52 + (x - '0'.to_int())
  } else if x == '+'.to_int() {
    62
  } else if x == '/'.to_int() {
    63
  } else {
    raise DecodeError("jsonschema_import: invalid binary base64 char")
  }
}

///|
fn jsonschema_import_base64_to_bytes(b64 : String) -> Bytes raise DecodeError {
  let len = b64.length()
  if len % 4 != 0 {
    raise DecodeError("jsonschema_import: invalid binary base64 length")
  }
  if len == 0 {
    return Bytes::new(0)
  }
  let eq = '='.to_int().to_uint16()
  let pad = if b64[len - 2] == eq {
    2
  } else if b64[len - 1] == eq {
    1
  } else {
    0
  }
  let out : Array[Byte] = []
  let full_end = len - (if pad > 0 { 4 } else { 0 })
  let mut i = 0
  while i < full_end {
    let v0 = jsonschema_import_base64_sextet(b64[i])
    let v1 = jsonschema_import_base64_sextet(b64[i + 1])
    let v2 = jsonschema_import_base64_sextet(b64[i + 2])
    let v3 = jsonschema_import_base64_sextet(b64[i + 3])
    let n = (v0 << 18) | (v1 << 12) | (v2 << 6) | v3
    out.push(((n >> 16) & 0xFF).to_byte())
    out.push(((n >> 8) & 0xFF).to_byte())
    out.push((n & 0xFF).to_byte())
    i = i + 4
  }
  if pad > 0 {
    let v0 = jsonschema_import_base64_sextet(b64[full_end])
    let v1 = jsonschema_import_base64_sextet(b64[full_end + 1])
    if pad == 2 {
      if b64[full_end + 3] != eq || (v1 & 0x0F) != 0 {
        raise DecodeError("jsonschema_import: invalid binary base64 padding")
      }
      out.push(((v0 << 2) | (v1 >> 4)).to_byte())
    } else {
      let v2 = jsonschema_import_base64_sextet(b64[full_end + 2])
      if (v2 & 0x03) != 0 {
        raise DecodeError("jsonschema_import: invalid binary base64 padding")
      }
      let n = (v0 << 18) | (v1 << 12) | (v2 << 6)
      out.push(((n >> 16) & 0xFF).to_byte())
      out.push(((n >> 8) & 0xFF).to_byte())
    }
  }
  Bytes::from_array(out)
}
