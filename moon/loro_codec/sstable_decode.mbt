///|
fn decode_block_meta(
  bytes : BytesView,
  check_checksum : Bool,
) -> Array[BlockMeta] raise DecodeError {
  let len = bytes.length()
  if len < 8 {
    raise DecodeError("sstable: invalid meta bytes")
  }
  if check_checksum {
    let stored = BytesReader::from_view(bytes[len - 4:len]).read_u32_le()
    let expected = xxhash32(bytes[4:len - 4], LORO_XXH32_SEED)
    if expected != stored {
      raise DecodeError("sstable: meta checksum mismatch")
    }
  }
  let r = BytesReader::from_view(bytes)
  let num = r.read_u32_le().reinterpret_as_int()
  if num < 0 || num > MAX_BLOCK_NUM {
    raise DecodeError("sstable: invalid block count")
  }
  let metas : Array[BlockMeta] = []
  for _i in 0..<num {
    let offset = r.read_u32_le().reinterpret_as_int()
    let first_len = r.read_u16_le().reinterpret_as_int()
    if first_len < 0 {
      raise DecodeError("sstable: invalid first key length")
    }
    let first_key = r.read_exact(first_len).to_bytes()
    let flags = r.read_u8().to_uint()
    let is_large = (flags & 0x80) != 0
    let compression_type = compression_type_from_u8(flags & 0x7F)
    if is_large {
      metas.push({
        offset,
        is_large,
        compression_type,
        first_key,
        last_key: None,
      })
      continue
    }
    let last_len = r.read_u16_le().reinterpret_as_int()
    if last_len < 0 {
      raise DecodeError("sstable: invalid last key length")
    }
    let last_key = r.read_exact(last_len).to_bytes()
    metas.push({
      offset,
      is_large,
      compression_type,
      first_key,
      last_key: Some(last_key),
    })
  }
  let _ = r.read_u32_le()
  if r.remaining() != 0 {
    raise DecodeError("sstable: trailing bytes in meta")
  }
  metas
}

///|
fn decode_normal_block(
  data : BytesView,
  first_key : Bytes,
) -> Array[(Bytes, Bytes)] raise DecodeError {
  if data.length() < 2 {
    raise DecodeError("sstable: invalid normal block")
  }
  let total_len = data.length()
  let kv_len = BytesReader::from_view(data[total_len - 2:total_len])
    .read_u16_le()
    .reinterpret_as_int()
  if kv_len < 0 {
    raise DecodeError("sstable: invalid kv_len")
  }
  let offsets_bytes_len = kv_len * 2
  let data_end = total_len - 2 - offsets_bytes_len
  if data_end < 0 {
    raise DecodeError("sstable: invalid offsets section")
  }
  let offsets_view = data[data_end:data_end + offsets_bytes_len]
  let offsets : Array[Int] = []
  for i in 0..<kv_len {
    let off = BytesReader::from_view(offsets_view[i * 2:i * 2 + 2])
      .read_u16_le()
      .reinterpret_as_int()
    offsets.push(off)
  }
  let kvs : Array[(Bytes, Bytes)] = []
  for i in 0..<kv_len {
    let start = offsets[i]
    let end = if i + 1 < kv_len { offsets[i + 1] } else { data_end }
    if start < 0 || end < start || end > data_end {
      raise DecodeError("sstable: invalid offset range")
    }
    if i == 0 {
      kvs.push((first_key, data[start:end].to_bytes()))
      continue
    }
    let rest = data[start:end]
    if rest.length() < 3 {
      raise DecodeError("sstable: invalid kv chunk")
    }
    let common = rest[0].to_int()
    if common < 0 || common > first_key.length() {
      raise DecodeError("sstable: invalid common prefix len")
    }
    let key_suffix_len = BytesReader::from_view(rest[1:3])
      .read_u16_le()
      .reinterpret_as_int()
    if key_suffix_len < 0 || 3 + key_suffix_len > rest.length() {
      raise DecodeError("sstable: invalid key suffix len")
    }
    let key_suffix = rest[3:3 + key_suffix_len]
    let value = rest[3 + key_suffix_len:]
    let w = BytesWriter::new()
    w.write_bytesview(first_key[0:common])
    w.write_bytesview(key_suffix)
    kvs.push((w.to_bytes(), value.to_bytes()))
  }
  kvs
}

///|
pub fn sstable_import_all(
  bytes : Bytes,
  check_checksum : Bool,
) -> Array[(Bytes, Bytes)] raise DecodeError {
  if bytes.length() < 9 {
    raise DecodeError("sstable: invalid bytes")
  }
  if bytes[0:4] != SSTABLE_MAGIC[:] {
    raise DecodeError("sstable: invalid magic")
  }
  if bytes[4] != CURRENT_SCHEMA_VERSION {
    raise DecodeError("sstable: invalid schema version")
  }
  let meta_offset = BytesReader::from_view(bytes[bytes.length() - 4:])
    .read_u32_le()
    .reinterpret_as_int()
  if meta_offset < 5 || meta_offset >= bytes.length() - 4 {
    raise DecodeError("sstable: invalid meta offset")
  }
  let raw_meta = bytes[meta_offset:bytes.length() - 4]
  let meta = decode_block_meta(raw_meta, check_checksum)
  let kvs : Array[(Bytes, Bytes)] = []
  for i in 0..<meta.length() {
    let m = meta[i]
    let offset = m.offset
    let offset_end = if i + 1 < meta.length() {
      meta[i + 1].offset
    } else {
      meta_offset
    }
    if offset < 5 || offset_end < offset || offset_end > meta_offset {
      raise DecodeError("sstable: invalid block offset")
    }
    let raw_block_and_check = bytes[offset:offset_end]
    if raw_block_and_check.length() < 4 {
      raise DecodeError("sstable: invalid block bytes")
    }
    let stored = BytesReader::from_view(
      raw_block_and_check[raw_block_and_check.length() - 4:],
    ).read_u32_le()
    let body = raw_block_and_check[0:raw_block_and_check.length() - 4]
    if check_checksum {
      let expected = xxhash32(body, LORO_XXH32_SEED)
      if expected != stored {
        raise DecodeError("sstable: block checksum mismatch")
      }
    }
    let uncompressed = match m.compression_type {
      CompressionType::None => body.to_bytes()
      CompressionType::LZ4 => lz4_decompress_frame(body, check_checksum)
    }
    if m.is_large {
      kvs.push((m.first_key, uncompressed))
    } else {
      let pairs = decode_normal_block(uncompressed[:], m.first_key)
      for p in pairs {
        kvs.push(p)
      }
    }
  }
  kvs
}
