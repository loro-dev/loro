///|
const SSTABLE_MAGIC : Bytes = b"LORO"

///|
const CURRENT_SCHEMA_VERSION : Byte = b'\x00'

///|
const MAX_BLOCK_NUM : Int = 10_000_000

///|
pub enum CompressionType {
  None
  LZ4
}

///|
fn compression_type_from_u8(v : UInt) -> CompressionType raise DecodeError {
  match v {
    0 => CompressionType::None
    1 => CompressionType::LZ4
    _ => raise DecodeError("sstable: invalid compression type")
  }
}

///|
fn compression_type_to_u8(v : CompressionType) -> Byte {
  match v {
    CompressionType::None => b'\x00'
    CompressionType::LZ4 => b'\x01'
  }
}

///|
pub struct BlockMeta {
  offset : Int
  is_large : Bool
  compression_type : CompressionType
  first_key : Bytes
  last_key : Bytes?
}

///|
fn bytes_compare(a : BytesView, b : BytesView) -> Int {
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

///|
fn common_prefix_len(a : Bytes, b : Bytes) -> Int {
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
