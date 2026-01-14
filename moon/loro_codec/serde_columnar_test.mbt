test "serde_columnar: outer format" {
  let cols: Array[Bytes] = [b"abc", b"\x01\x02"]
  let encoded = encode_columnar_vec(cols)
  assert_eq(encoded, b"\x02\x03abc\x02\x01\x02")
  let decoded = try! decode_columnar_vec(encoded[:])
  assert_eq(decoded.length(), 2)
  assert_eq(decoded[0].to_bytes(), b"abc")
  assert_eq(decoded[1].to_bytes(), b"\x01\x02")
}

test "serde_columnar: boolrle decode example" {
  // [T, T, F, F, F, T] -> [0, 2, 3, 1]
  let decoded = try! decode_bool_rle(b"\x00\x02\x03\x01"[:])
  assert_eq(decoded, [true, true, false, false, false, true])
}

test "serde_columnar: boolrle encode/decode" {
  let cases: Array[Array[Bool]] = [
    [],
    [false, false, true],
    [true, true, false, false, false, true],
    [true, true, true, true, true],
    [false, false, false],
  ]
  for v in cases {
    let enc = encode_bool_rle(v)
    let dec = try! decode_bool_rle(enc[:])
    assert_eq(dec, v)
  }
}

test "serde_columnar: anyrle u8 decode example" {
  // [5,5,5,3,3] -> [0x06,0x05,0x04,0x03]
  let decoded = try! decode_rle_u8(b"\x06\x05\x04\x03"[:])
  assert_eq(decoded, [5, 5, 5, 3, 3])
}

test "serde_columnar: anyrle u32 literal example" {
  // [1,2,3] literal => length=-3 zigzag(-3)=5 => [0x05, 0x01, 0x02, 0x03]
  let decoded = try! decode_any_rle_u32(b"\x05\x01\x02\x03"[:])
  assert_eq(decoded, [1, 2, 3])
}

test "serde_columnar: deltarle u32 decode example" {
  // Values [10, 11, 12, 13, 15, 17]
  // Deltas [10,1,1,1,2,2] encoded as AnyRle<i128>:
  // run(1,10), run(3,1), run(2,2) =>
  // [zigzag(1)=2, zigzag(10)=20, zigzag(3)=6, zigzag(1)=2, zigzag(2)=4, zigzag(2)=4]
  let decoded = try! decode_delta_rle_u32(b"\x02\x14\x06\x02\x04\x04"[:])
  assert_eq(decoded, [10, 11, 12, 13, 15, 17])
}

test "serde_columnar: deltarle encode/decode" {
  let values: Array[UInt] = [10, 11, 12, 13, 15, 17]
  let enc = encode_delta_rle_u32(values)
  let dec = try! decode_delta_rle_u32(enc[:])
  assert_eq(dec, values)
}

test "serde_columnar: delta_of_delta encode/decode empty" {
  let enc = encode_delta_of_delta_i64([])
  assert_eq(enc, b"\x00\x00")
  let dec = try! decode_delta_of_delta_i64(enc[:])
  assert_eq(dec, [])
}

test "serde_columnar: delta_of_delta encode/decode single" {
  let enc = encode_delta_of_delta_i64([5L])
  assert_eq(enc, b"\x01\x0A\x00")
  let dec = try! decode_delta_of_delta_i64(enc[:])
  assert_eq(dec, [5L])
}

test "serde_columnar: delta_of_delta known bytes" {
  // Values [1,2,3]:
  // first=1 => Option::Some(1) => [0x01, zigzag(1)=0x02]
  // delta1=1 => prefix 10 + (1+63)=64 => bits "10 1000000"
  // delta2=1 => dod=0 => bit "0"
  // bitstream bits: 10100000 00...... => bytes [0xA0, 0x00], last_used_bits=2
  let encoded = b"\x01\x02\x02\xA0\x00"
  let decoded = try! decode_delta_of_delta_i64(encoded[:])
  assert_eq(decoded, [1L, 2L, 3L])

  let re = encode_delta_of_delta_i64([1L, 2L, 3L])
  assert_eq(re, encoded)
}
