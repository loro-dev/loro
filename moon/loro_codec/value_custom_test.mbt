fn assert_roundtrip(bytes: Bytes) -> Unit raise {
  let (v, rest) = decode_value(bytes[:]) catch { DecodeError(msg) =>
    fail(msg)
  }
  assert_eq(rest.length(), 0)
  let encoded = encode_value(v)
  assert_eq(encoded, bytes)
}

test "value_custom: primitives roundtrip" {
  assert_roundtrip(b"\x00") // Null
  assert_roundtrip(b"\x01") // True
  assert_roundtrip(b"\x02") // False
  assert_roundtrip(b"\x03\x7F") // I64(-1)
  assert_roundtrip(b"\x0A\x7F") // DeltaInt(-1)
  assert_roundtrip(b"\x05\x02hi") // Str("hi")
  assert_roundtrip(b"\x06\x03\x01\x02\x03") // Binary([1,2,3])
  assert_roundtrip(b"\x07\xAC\x02") // ContainerIdx(300)
  assert_roundtrip(b"\x08") // DeleteOnce
  assert_roundtrip(b"\x09") // DeleteSeq
}

test "value_custom: f64 big-endian roundtrip" {
  // 1.0 in IEEE 754 double (big-endian)
  assert_roundtrip(b"\x04\x3F\xF0\x00\x00\x00\x00\x00\x00")
}

test "value_custom: nested loro_value list/map" {
  // Value::LoroValue(List([I64(1), Str("x")]))
  assert_roundtrip(b"\x0B\x07\x02\x03\x01\x05\x01x")
  // Value::LoroValue(Map({0: Null}))
  assert_roundtrip(b"\x0B\x08\x01\x00\x00")
}

test "value_custom: mark/tree/list ops" {
  // MarkStart(info=0x84, len=1, key_idx=0, value=True)
  assert_roundtrip(b"\x0C\x84\x01\x00\x01")
  // TreeMove(target=1, parent=null, position=2)
  assert_roundtrip(b"\x0D\x01\x01\x02")
  // TreeMove(target=1, parent=3, position=2)
  assert_roundtrip(b"\x0D\x01\x00\x02\x03")
  // ListMove(from=1, from_idx=2, lamport=3)
  assert_roundtrip(b"\x0E\x01\x02\x03")
  // ListSet(peer_idx=7, lamport=42, value=I64(1))
  assert_roundtrip(b"\x0F\x07\x2A\x03\x01")
}

test "value_custom: raw_tree_move" {
  // RawTreeMove(subject_peer=1, subject_cnt=2, position=3, parent=null)
  assert_roundtrip(b"\x10\x01\x02\x03\x01")
  // RawTreeMove(subject_peer=1, subject_cnt=2, position=3, parent=(4,5))
  assert_roundtrip(b"\x10\x01\x02\x03\x00\x04\x05")
}

test "value_custom: future unknown kind payload" {
  // Future tag byte + binary payload (uleb128 len + bytes)
  assert_roundtrip(b"\x91\x01X")
}
