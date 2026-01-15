///|
const LZ4_MAGIC : UInt = 0x184D2204

///|
fn lz4_header_checksum(descriptor : BytesView) -> Byte raise DecodeError {
  // LZ4 Frame spec: header checksum is xxHash32(descriptor, seed=0) >> 8 & 0xFF
  let h = xxhash32(descriptor, 0)
  ((h >> 8) & 0xFF).to_byte()
}

///|
fn lz4_block_checksum(block_data : BytesView) -> UInt raise DecodeError {
  xxhash32(block_data, 0)
}

///|
fn read_ext_len(r : BytesReader, base : Int) -> Int raise DecodeError {
  let mut len = base
  if base != 15 {
    return len
  }
  while true {
    let b = r.read_u8().to_int()
    len = len + b
    if b != 255 {
      break
    }
  }
  len
}

///|
fn lz4_decompress_block_into(
  out : @buffer.Buffer,
  input : BytesView,
) -> Unit raise DecodeError {
  let r = BytesReader::from_view(input)
  while r.remaining() > 0 {
    let token = r.read_u8().to_uint()
    let literal_base = ((token >> 4) & 0x0F).reinterpret_as_int()
    let literal_len = read_ext_len(r, literal_base)
    if literal_len < 0 {
      raise DecodeError("lz4: invalid literal length")
    }
    out.write_bytesview(r.read_exact(literal_len))
    if r.remaining() == 0 {
      break
    }
    let offset = r.read_u16_le().reinterpret_as_int()
    if offset <= 0 {
      raise DecodeError("lz4: invalid match offset")
    }
    if offset > out.length() {
      raise DecodeError("lz4: match offset out of bounds")
    }
    let match_base = (token & 0x0F).reinterpret_as_int()
    let mut match_len = match_base + 4
    if match_base == 15 {
      match_len = match_len + read_ext_len(r, 15) - 15
    }
    if match_len < 0 {
      raise DecodeError("lz4: invalid match length")
    }
    let start = out.length() - offset
    for i in 0..<match_len {
      // NOTE: This supports overlap copy because out grows as we write.
      let b = out.view()[start + i]
      out.write_byte(b)
    }
  }
}

///|
fn lz4_decompress_block(input : BytesView) -> Bytes raise DecodeError {
  let out = @buffer.new()
  lz4_decompress_block_into(out, input)
  out.to_bytes()
}

///|
fn block_max_size_from_bd(bd : Byte) -> Int raise DecodeError {
  if (bd.to_uint() & 0x8F) != 0 {
    raise DecodeError("lz4: invalid BD")
  }
  let id = ((bd.to_uint() >> 4) & 0x07).reinterpret_as_int()
  match id {
    4 => 64 * 1024
    5 => 256 * 1024
    6 => 1 * 1024 * 1024
    7 => 4 * 1024 * 1024
    _ => raise DecodeError("lz4: unsupported block max size")
  }
}

///|
pub fn lz4_decompress_frame(
  bytes : BytesView,
  check_checksum : Bool,
) -> Bytes raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let magic = r.read_u32_le()
  if magic != LZ4_MAGIC {
    raise DecodeError("lz4: invalid magic")
  }
  let flg = r.read_u8()
  let version = ((flg.to_uint() >> 6) & 0x03).reinterpret_as_int()
  if version != 1 {
    raise DecodeError("lz4: unsupported version")
  }
  if (flg.to_uint() & 0x02) != 0 {
    raise DecodeError("lz4: invalid FLG (reserved bit set)")
  }
  let block_independence = (flg.to_uint() & 0x20) != 0
  let block_checksum_flag = (flg.to_uint() & 0x10) != 0
  let content_size_flag = (flg.to_uint() & 0x08) != 0
  let content_checksum_flag = (flg.to_uint() & 0x04) != 0
  let dict_id_flag = (flg.to_uint() & 0x01) != 0
  let bd = r.read_u8()
  let block_max_size = block_max_size_from_bd(bd)
  let desc = BytesWriter::new()
  desc.write_u8(flg)
  desc.write_u8(bd)
  let mut expected_content_size : UInt64? = None
  if content_size_flag {
    let size = r.read_u64_le()
    expected_content_size = Some(size)
    desc.write_u64_le(size)
  }
  if dict_id_flag {
    let dict_id = r.read_u32_le()
    desc.write_u32_le(dict_id)
  }
  let header_checksum = r.read_u8()
  if check_checksum {
    let expected = lz4_header_checksum(desc.to_bytes()[:])
    if expected != header_checksum {
      raise DecodeError("lz4: header checksum mismatch")
    }
  }
  let out = @buffer.new()
  while true {
    let block_size_raw = r.read_u32_le()
    if block_size_raw == 0 {
      break
    }
    let is_uncompressed = (block_size_raw & 0x80000000) != 0
    let size = (block_size_raw & 0x7FFFFFFF).reinterpret_as_int()
    if size < 0 || size > block_max_size {
      raise DecodeError("lz4: invalid block size")
    }
    let block_data = r.read_exact(size)
    if block_checksum_flag {
      let stored = r.read_u32_le()
      if check_checksum {
        let expected = lz4_block_checksum(block_data)
        if expected != stored {
          raise DecodeError("lz4: block checksum mismatch")
        }
      }
    }
    if is_uncompressed {
      out.write_bytesview(block_data)
    } else if block_independence {
      out.write_bytes(lz4_decompress_block(block_data))
    } else {
      lz4_decompress_block_into(out, block_data)
    }
  }
  if content_checksum_flag {
    let stored = r.read_u32_le()
    if check_checksum {
      let expected = xxhash32(out.to_bytes()[:], 0)
      if expected != stored {
        raise DecodeError("lz4: content checksum mismatch")
      }
    }
  }
  if check_checksum {
    match expected_content_size {
      Some(sz) =>
        if out.length().to_uint64() != sz {
          raise DecodeError("lz4: content size mismatch")
        }
      None => ()
    }
  }
  if r.remaining() != 0 {
    raise DecodeError("lz4: trailing bytes")
  }
  out.to_bytes()
}

///|
fn write_ext_len(w : BytesWriter, extra : Int) -> Unit raise EncodeError {
  if extra < 0 {
    raise EncodeError("lz4: negative ext len")
  }
  let mut left = extra
  while left >= 255 {
    w.write_u8(255)
    left = left - 255
  }
  w.write_u8(left.to_byte())
}

///|
fn read_u32_le_at(input : BytesView, i : Int) -> UInt {
  let b0 = input[i].to_uint()
  let b1 = input[i + 1].to_uint()
  let b2 = input[i + 2].to_uint()
  let b3 = input[i + 3].to_uint()
  b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
}

///|
fn bytes_eq4(input : BytesView, a : Int, b : Int) -> Bool {
  input[a] == input[b] &&
  input[a + 1] == input[b + 1] &&
  input[a + 2] == input[b + 2] &&
  input[a + 3] == input[b + 3]
}

///|
fn hash_u32(x : UInt) -> Int {
  // LZ4 uses a multiplicative hash for 4-byte sequences.
  let mul = x.to_uint64() * 2654435761UL
  ((mul >> 16) & 0xFFFFUL).to_int()
}

///|
fn lz4_compress_block(input : BytesView) -> Bytes raise EncodeError {
  let n = input.length()
  if n == 0 {
    return b""
  }

  // Hash table for 4-byte sequences. Use -1 as empty.
  let table : Array[Int] = []
  for _i in 0..<65536 {
    table.push(-1)
  }

  let w = BytesWriter::new()
  let mut anchor = 0
  let mut i = 0
  while i + 4 <= n {
    let seq = read_u32_le_at(input, i)
    let h = hash_u32(seq)
    let ref_pos = table[h]
    table[h] = i

    if ref_pos >= 0 && i - ref_pos <= 65535 &&
      bytes_eq4(input, ref_pos, i) {
      // Extend match.
      let mut mlen = 4
      while i + mlen < n && input[ref_pos + mlen] == input[i + mlen] {
        mlen = mlen + 1
      }

      let lit_len = i - anchor
      let match_len_minus4 = mlen - 4
      let lit_n = if lit_len < 15 { lit_len } else { 15 }
      let match_n = if match_len_minus4 < 15 {
        match_len_minus4
      } else {
        15
      }
      w.write_u8(((lit_n << 4) | match_n).to_byte())
      if lit_len >= 15 {
        write_ext_len(w, lit_len - 15)
      }
      w.write_bytesview(input[anchor:i])

      let offset = i - ref_pos
      if offset <= 0 || offset > 65535 {
        raise EncodeError("lz4: invalid match offset")
      }
      w.write_u16_le(offset.reinterpret_as_uint())

      if match_len_minus4 >= 15 {
        write_ext_len(w, match_len_minus4 - 15)
      }

      i = i + mlen
      anchor = i

      // Prime the table at i-2 for better compression, like the reference impl.
      if i - 2 >= 0 && i - 2 + 4 <= n {
        let seq2 = read_u32_le_at(input, i - 2)
        table[hash_u32(seq2)] = i - 2
      }
      continue
    }

    i = i + 1
  }

  // Last literals.
  let lit_len = n - anchor
  let lit_n = if lit_len < 15 { lit_len } else { 15 }
  w.write_u8((lit_n << 4).to_byte())
  if lit_len >= 15 {
    write_ext_len(w, lit_len - 15)
  }
  w.write_bytesview(input[anchor:n])
  w.to_bytes()
}

///|
pub fn lz4_compress_frame(bytes : Bytes) -> Bytes raise EncodeError {
  // Emit a minimal LZ4 frame (block independent, no checksums/content size),
  // compatible with Rust's `lz4_flex::frame::FrameDecoder`.
  let flg = b'\x60' // version=1, block_independence=1
  let bd = b'\x40' // block_max_size=64KB

  let desc = BytesWriter::new()
  desc.write_u8(flg)
  desc.write_u8(bd)
  let header_checksum = lz4_header_checksum(desc.to_bytes()[:]) catch {
    DecodeError(e) => raise EncodeError("lz4: header checksum failed: " + e)
  }

  let w = BytesWriter::new()
  w.write_u32_le(LZ4_MAGIC)
  w.write_u8(flg)
  w.write_u8(bd)
  w.write_u8(header_checksum)

  let block_max = 64 * 1024
  let mut pos = 0
  while pos < bytes.length() {
    let remaining = bytes.length() - pos
    let len = if remaining > block_max { block_max } else { remaining }
    let chunk = bytes[pos:pos + len]
    let compressed = lz4_compress_block(chunk)
    if compressed.length() >= len {
      // Use an uncompressed block inside the frame.
      let raw = 0x8000_0000U | len.reinterpret_as_uint()
      w.write_u32_le(raw)
      w.write_bytesview(chunk)
    } else {
      w.write_u32_le(compressed.length().reinterpret_as_uint())
      w.write_bytes(compressed)
    }
    pos = pos + len
  }

  // End mark.
  w.write_u32_le(0)
  w.to_bytes()
}
