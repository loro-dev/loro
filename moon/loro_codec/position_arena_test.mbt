test "position_arena: v2 roundtrip" {
  let positions: Array[Bytes] = [
    b"\x01\x02\x03",
    b"\x01\x02\x04",
    b"\x01\x02\x04\xFF",
    b"\x01",
    b"",
  ]

  let encoded = encode_position_arena_v2(positions)
  let decoded = try! decode_position_arena_v2(encoded[:])
  assert_eq(decoded.length(), positions.length())
  for i in 0..<positions.length() {
    assert_eq(decoded[i], positions[i])
  }
}

test "position_arena: empty bytes means empty" {
  let decoded = try! decode_position_arena_v2(b""[:])
  assert_eq(decoded.length(), 0)
}

