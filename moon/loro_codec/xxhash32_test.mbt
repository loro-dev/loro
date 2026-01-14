test "xxhash32 test vectors" {
  fn h(data: Bytes, seed: UInt) -> UInt {
    xxhash32(data[:], seed) catch { DecodeError(_) => 0 }
  }

  assert_eq(h(b"", 0), 0x02CC5D05)

  let seed = LORO_XXH32_SEED
  assert_eq(h(b"", seed), 0xDC3BF95A)
  assert_eq(h(b"\x00", seed), 0xDAD9F666)
  assert_eq(h(b"loro", seed), 0x74D321EA)
  assert_eq(
    h(
      b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0A\x0B\x0C\x0D\x0E\x0F",
      seed,
    ),
    0x2EDAB25F,
  )
}
