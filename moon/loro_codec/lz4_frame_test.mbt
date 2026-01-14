test "lz4 frame: uncompressed block (Hello, World!)" {
  let input = b"Hello, World!"
  let frame = b"\x04\x22\x4D\x18\x60\x40\x82\x0D\x00\x00\x80Hello, World!\x00\x00\x00\x00"
  let out = lz4_decompress_frame(frame[:], true) catch { DecodeError(_) => b"" }
  assert_eq(out, input)
}

test "lz4 frame: header checksum mismatch" {
  let frame = b"\x04\x22\x4D\x18\x60\x40\x83\x00\x00\x00\x00"
  let out = lz4_decompress_frame(frame[:], true) catch { DecodeError(_) => b"ERR" }
  assert_eq(out, b"ERR")
}

test "lz4 frame: compressed block (simple match copy)" {
  // Decompresses to "abcabcabc".
  let payload = b"\x32abc\x03\x00"
  let w = BytesWriter::new()
  w.write_u32_le(0x184D2204)
  w.write_u8(0x60)
  w.write_u8(0x40)
  w.write_u8(0x82)
  w.write_u32_le(payload.length().reinterpret_as_uint())
  w.write_bytes(payload)
  w.write_u32_le(0)
  let frame = w.to_bytes()
  let out = lz4_decompress_frame(frame[:], true) catch { DecodeError(_) => b"" }
  assert_eq(out, b"abcabcabc")
}

test "lz4 frame: compressed block overlap copy" {
  // Decompresses to 10x 'a'.
  let payload = b"\x15a\x01\x00"
  let w = BytesWriter::new()
  w.write_u32_le(0x184D2204)
  w.write_u8(0x60)
  w.write_u8(0x40)
  w.write_u8(0x82)
  w.write_u32_le(payload.length().reinterpret_as_uint())
  w.write_bytes(payload)
  w.write_u32_le(0)
  let frame = w.to_bytes()
  let out = lz4_decompress_frame(frame[:], true) catch { DecodeError(_) => b"" }
  assert_eq(out, b"aaaaaaaaaa")
}
