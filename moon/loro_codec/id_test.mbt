test "change block key encode/decode" {
  let id = ID::new(0x0102030405060708, -1)
  let key = id.to_change_block_key()
  assert_eq(key, b"\x01\x02\x03\x04\x05\x06\x07\x08\xFF\xFF\xFF\xFF")

  let parsed = try! id_from_change_block_key(key[:])
  assert_eq(parsed.peer(), 0x0102030405060708)
  assert_eq(parsed.counter(), -1)
}

