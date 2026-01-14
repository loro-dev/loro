test "change_block: postcard encoded block roundtrip" {
  // u32 fields: 1,2,10,2,1; then 8 bytes fields with varint len.
  let encoded = b"\x01\x02\x0A\x02\x01\x03hdr\x04meta\x01c\x01k\x01p\x01o\x01d\x01v"

  let decoded = try! decode_encoded_block(encoded[:])
  assert_eq(decoded.counter_start(), 1)
  assert_eq(decoded.counter_len(), 2)
  assert_eq(decoded.lamport_start(), 10)
  assert_eq(decoded.lamport_len(), 2)
  assert_eq(decoded.n_changes(), 1)
  assert_eq(decoded.header(), b"hdr")
  assert_eq(decoded.change_meta(), b"meta")
  assert_eq(decoded.cids(), b"c")
  assert_eq(decoded.keys(), b"k")
  assert_eq(decoded.positions(), b"p")
  assert_eq(decoded.ops(), b"o")
  assert_eq(decoded.delete_start_ids(), b"d")
  assert_eq(decoded.values(), b"v")

  let encoded2 = encode_encoded_block(decoded)
  assert_eq(encoded2, encoded)
}
