const SSTABLE_MAGIC: Bytes = b"LORO"
const CURRENT_SCHEMA_VERSION: Byte = b'\x00'
const MAX_BLOCK_NUM: Int = 10_000_000

pub enum CompressionType {
  None
  LZ4
}

fn compression_type_from_u8(v: UInt) -> CompressionType raise DecodeError {
  match v {
    0 => CompressionType::None
    1 => CompressionType::LZ4
    _ => raise DecodeError("sstable: invalid compression type")
  }
}

fn compression_type_to_u8(v: CompressionType) -> Byte {
  match v {
    CompressionType::None => b'\x00'
    CompressionType::LZ4 => b'\x01'
  }
}

pub struct BlockMeta {
  offset: Int
  is_large: Bool
  compression_type: CompressionType
  first_key: Bytes
  last_key: Bytes?
}

fn bytes_compare(a: BytesView, b: BytesView) -> Int {
  let n = if a.length() < b.length() { a.length() } else { b.length() }
  for i in 0..<n {
    let ai = a[i].to_int()
    let bi = b[i].to_int()
    if ai != bi {
      return ai - bi
    }
  }
  a.length() - b.length()
}

fn common_prefix_len(a: Bytes, b: Bytes) -> Int {
  let n = if a.length() < b.length() { a.length() } else { b.length() }
  let mut i = 0
  while i < n && i < 255 {
    if a[i] != b[i] {
      break
    }
    i = i + 1
  }
  i
}

fn decode_block_meta(bytes: BytesView, check_checksum: Bool) -> Array[BlockMeta] raise DecodeError {
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

  let metas: Array[BlockMeta] = []
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
      metas.push(
        {
          offset,
          is_large,
          compression_type,
          first_key,
          last_key: None,
        },
      )
      continue
    }
    let last_len = r.read_u16_le().reinterpret_as_int()
    if last_len < 0 {
      raise DecodeError("sstable: invalid last key length")
    }
    let last_key = r.read_exact(last_len).to_bytes()
    metas.push(
      {
        offset,
        is_large,
        compression_type,
        first_key,
        last_key: Some(last_key),
      },
    )
  }

  let _ = r.read_u32_le()
  if r.remaining() != 0 {
    raise DecodeError("sstable: trailing bytes in meta")
  }
  metas
}

fn decode_normal_block(data: BytesView, first_key: Bytes) -> Array[(Bytes, Bytes)] raise DecodeError {
  if data.length() < 2 {
    raise DecodeError("sstable: invalid normal block")
  }
  let total_len = data.length()
  let kv_len = BytesReader::from_view(data[total_len - 2:total_len]).read_u16_le().reinterpret_as_int()
  if kv_len < 0 {
    raise DecodeError("sstable: invalid kv_len")
  }
  let offsets_bytes_len = kv_len * 2
  let data_end = total_len - 2 - offsets_bytes_len
  if data_end < 0 {
    raise DecodeError("sstable: invalid offsets section")
  }
  let offsets_view = data[data_end:data_end + offsets_bytes_len]
  let offsets: Array[Int] = []
  for i in 0..<kv_len {
    let off = BytesReader::from_view(offsets_view[i * 2:i * 2 + 2]).read_u16_le().reinterpret_as_int()
    offsets.push(off)
  }

  let kvs: Array[(Bytes, Bytes)] = []
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
    let key_suffix_len = BytesReader::from_view(rest[1:3]).read_u16_le().reinterpret_as_int()
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

pub fn sstable_import_all(bytes: Bytes, check_checksum: Bool) -> Array[(Bytes, Bytes)] raise DecodeError {
  if bytes.length() < 9 {
    raise DecodeError("sstable: invalid bytes")
  }
  if bytes[0:4] != SSTABLE_MAGIC[:] {
    raise DecodeError("sstable: invalid magic")
  }
  if bytes[4] != CURRENT_SCHEMA_VERSION {
    raise DecodeError("sstable: invalid schema version")
  }

  let meta_offset = BytesReader::from_view(bytes[bytes.length() - 4:]).read_u32_le().reinterpret_as_int()
  if meta_offset < 5 || meta_offset >= bytes.length() - 4 {
    raise DecodeError("sstable: invalid meta offset")
  }
  let raw_meta = bytes[meta_offset:bytes.length() - 4]
  let meta = decode_block_meta(raw_meta, check_checksum)

  let kvs: Array[(Bytes, Bytes)] = []
  for i in 0..<meta.length() {
    let m = meta[i]
    let offset = m.offset
    let offset_end = if i + 1 < meta.length() { meta[i + 1].offset } else { meta_offset }
    if offset < 5 || offset_end < offset || offset_end > meta_offset {
      raise DecodeError("sstable: invalid block offset")
    }
    let raw_block_and_check = bytes[offset:offset_end]
    if raw_block_and_check.length() < 4 {
      raise DecodeError("sstable: invalid block bytes")
    }
    let stored = BytesReader::from_view(raw_block_and_check[raw_block_and_check.length() - 4:]).read_u32_le()
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

pub fn sstable_export_all(
  kvs: Array[(Bytes, Bytes)],
  block_size: Int,
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

  let blocks: Array[Bytes] = []
  let meta: Array[BlockMeta] = []
  let mut next_offset = 5

  let mut idx = 0
  while idx < kvs.length() {
    let (key, value) = kvs[idx]

    if value.length() > block_size {
      let checksum = xxhash32(value[:], LORO_XXH32_SEED) catch { DecodeError(e) =>
        raise EncodeError("sstable: checksum failed: " + e)
      }
      let w = BytesWriter::new()
      w.write_bytes(value)
      w.write_u32_le(checksum)
      let block_bytes = w.to_bytes()
      blocks.push(block_bytes)
      meta.push(
        {
          offset: next_offset,
          is_large: true,
          compression_type: CompressionType::None,
          first_key: key,
          last_key: None,
        },
      )
      next_offset = next_offset + block_bytes.length()
      idx = idx + 1
      continue
    }

    let first_key = key
    let mut last_key = key
    let data_w = BytesWriter::new()
    let offsets: Array[Int] = []
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
    let checksum = xxhash32(body_bytes[:], LORO_XXH32_SEED) catch { DecodeError(e) =>
      raise EncodeError("sstable: checksum failed: " + e)
    }
    let w = BytesWriter::new()
    w.write_bytes(body_bytes)
    w.write_u32_le(checksum)
    let block_bytes = w.to_bytes()
    blocks.push(block_bytes)
    meta.push(
      {
        offset: next_offset,
        is_large: false,
        compression_type: CompressionType::None,
        first_key,
        last_key: Some(last_key),
      },
    )
    next_offset = next_offset + block_bytes.length()
  }

  let meta_offset = next_offset

  let meta_w = BytesWriter::new()
  meta_w.write_u32_le(meta.length().reinterpret_as_uint())
  for m in meta {
    meta_w.write_u32_le(m.offset.reinterpret_as_uint())
    meta_w.write_u16_le(m.first_key.length().reinterpret_as_uint())
    meta_w.write_bytes(m.first_key)
    let flags =
      (if m.is_large { 0x80 } else { 0 }).reinterpret_as_uint()
      | compression_type_to_u8(m.compression_type).to_uint()
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

