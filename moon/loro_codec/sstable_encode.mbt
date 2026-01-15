///|
pub fn sstable_export_all(
  kvs : Array[(Bytes, Bytes)],
  block_size : Int,
) -> Bytes raise EncodeError {
  if block_size <= 0 || block_size > 0xFFFF {
    raise EncodeError("sstable: invalid block_size")
  }
  for i in 0..<kvs.length() {
    let (k, _) = kvs[i]
    if k.length() == 0 {
      raise EncodeError("sstable: empty key")
    }
    if i > 0 {
      let (prev, _) = kvs[i - 1]
      if bytes_compare(prev[:], k[:]) > 0 {
        raise EncodeError("sstable: keys must be sorted")
      }
    }
  }
  let blocks : Array[Bytes] = []
  let meta : Array[BlockMeta] = []
  let mut next_offset = 5
  let mut idx = 0
  while idx < kvs.length() {
    let (key, value) = kvs[idx]
    if value.length() > block_size {
      let checksum = xxhash32(value[:], LORO_XXH32_SEED) catch {
        DecodeError(e) => raise EncodeError("sstable: checksum failed: " + e)
      }
      let w = BytesWriter::new()
      w.write_bytes(value)
      w.write_u32_le(checksum)
      let block_bytes = w.to_bytes()
      blocks.push(block_bytes)
      meta.push({
        offset: next_offset,
        is_large: true,
        compression_type: CompressionType::None,
        first_key: key,
        last_key: None,
      })
      next_offset = next_offset + block_bytes.length()
      idx = idx + 1
      continue
    }
    let first_key = key
    let mut last_key = key
    let data_w = BytesWriter::new()
    let offsets : Array[Int] = []
    let mut data_len = 0
    offsets.push(data_len)
    data_w.write_bytes(value)
    data_len = data_len + value.length()
    idx = idx + 1
    while idx < kvs.length() {
      let (k2, v2) = kvs[idx]
      if v2.length() > block_size {
        break
      }
      let estimated = 2 + offsets.length() * 2 + data_len + 4
      let add_est = k2.length() + v2.length() + 1 + 2
      if estimated + add_est > block_size {
        break
      }
      let common = common_prefix_len(first_key, k2)
      let suffix = k2[common:]
      let suffix_len = suffix.length()
      if suffix_len > 0xFFFF {
        raise EncodeError("sstable: key too long")
      }
      if data_len > 0xFFFF {
        raise EncodeError("sstable: block too large")
      }
      offsets.push(data_len)
      data_w.write_u8(common.to_byte())
      data_w.write_u16_le(suffix_len.reinterpret_as_uint())
      data_w.write_bytesview(suffix)
      data_w.write_bytes(v2)
      data_len = data_len + 1 + 2 + suffix_len + v2.length()
      last_key = k2
      idx = idx + 1
    }
    let body_w = BytesWriter::new()
    body_w.write_bytes(data_w.to_bytes())
    for off in offsets {
      if off < 0 || off > 0xFFFF {
        raise EncodeError("sstable: invalid offset")
      }
      body_w.write_u16_le(off.reinterpret_as_uint())
    }
    body_w.write_u16_le(offsets.length().reinterpret_as_uint())
    let body_bytes = body_w.to_bytes()
    let checksum = xxhash32(body_bytes[:], LORO_XXH32_SEED) catch {
      DecodeError(e) => raise EncodeError("sstable: checksum failed: " + e)
    }
    let w = BytesWriter::new()
    w.write_bytes(body_bytes)
    w.write_u32_le(checksum)
    let block_bytes = w.to_bytes()
    blocks.push(block_bytes)
    meta.push({
      offset: next_offset,
      is_large: false,
      compression_type: CompressionType::None,
      first_key,
      last_key: Some(last_key),
    })
    next_offset = next_offset + block_bytes.length()
  }
  let meta_offset = next_offset
  let meta_w = BytesWriter::new()
  meta_w.write_u32_le(meta.length().reinterpret_as_uint())
  for m in meta {
    meta_w.write_u32_le(m.offset.reinterpret_as_uint())
    meta_w.write_u16_le(m.first_key.length().reinterpret_as_uint())
    meta_w.write_bytes(m.first_key)
    let flags = (if m.is_large { 0x80 } else { 0 }).reinterpret_as_uint() |
      compression_type_to_u8(m.compression_type).to_uint()
    meta_w.write_u8((flags & 0xFF).to_byte())
    if !m.is_large {
      match m.last_key {
        Some(last_key) => {
          meta_w.write_u16_le(last_key.length().reinterpret_as_uint())
          meta_w.write_bytes(last_key)
        }
        None => raise EncodeError("sstable: missing last_key")
      }
    }
  }
  let meta_bytes_without_checksum = meta_w.to_bytes()
  let meta_checksum = xxhash32(meta_bytes_without_checksum[4:], LORO_XXH32_SEED) catch {
    DecodeError(e) => raise EncodeError("sstable: checksum failed: " + e)
  }
  meta_w.write_u32_le(meta_checksum)
  let meta_bytes = meta_w.to_bytes()
  let out = BytesWriter::new()
  out.write_bytes(SSTABLE_MAGIC)
  out.write_u8(CURRENT_SCHEMA_VERSION)
  for b in blocks {
    out.write_bytes(b)
  }
  out.write_bytes(meta_bytes)
  out.write_u32_le(meta_offset.reinterpret_as_uint())
  out.to_bytes()
}
