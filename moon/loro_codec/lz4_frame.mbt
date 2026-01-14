const LZ4_MAGIC: UInt = 0x184D2204

fn lz4_header_checksum(descriptor: BytesView) -> Byte raise DecodeError {
  // LZ4 Frame spec: header checksum is xxHash32(descriptor, seed=0) >> 8 & 0xFF
  let h = xxhash32(descriptor, 0)
  ((h >> 8) & 0xFF).to_byte()
}

fn lz4_block_checksum(block_data: BytesView) -> UInt raise DecodeError {
  xxhash32(block_data, 0)
}

fn read_ext_len(r: BytesReader, base: Int) -> Int raise DecodeError {
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

fn lz4_decompress_block_into(out: @buffer.Buffer, input: BytesView) -> Unit raise DecodeError {
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

fn lz4_decompress_block(input: BytesView) -> Bytes raise DecodeError {
  let out = @buffer.new()
  lz4_decompress_block_into(out, input)
  out.to_bytes()
}

fn block_max_size_from_bd(bd: Byte) -> Int raise DecodeError {
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

pub fn lz4_decompress_frame(bytes: BytesView, check_checksum: Bool) -> Bytes raise DecodeError {
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

  let mut expected_content_size: UInt64? = None
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
    } else {
      if block_independence {
        out.write_bytes(lz4_decompress_block(block_data))
      } else {
        lz4_decompress_block_into(out, block_data)
      }
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
      Some(sz) => {
        if out.length().to_uint64() != sz {
          raise DecodeError("lz4: content size mismatch")
        }
      }
      None => ()
    }
  }

  if r.remaining() != 0 {
    raise DecodeError("lz4: trailing bytes")
  }

  out.to_bytes()
}
