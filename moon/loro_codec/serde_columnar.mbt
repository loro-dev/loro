// Serde columnar outer format helpers.
//
// Strategies are split into separate files:
// - serde_columnar_bool_rle.mbt
// - serde_columnar_any_rle.mbt
// - serde_columnar_delta_rle.mbt
// - serde_columnar_delta_of_delta.mbt

pub fn take_columnar_vec(bytes: BytesView) -> (Array[BytesView], BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let n_cols_u64 = r.read_varint_u64()
  if n_cols_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("serde_columnar: too many columns")
  }
  let n_cols = n_cols_u64.to_int()
  let cols: Array[BytesView] = []
  for _i in 0..<n_cols {
    let len_u64 = r.read_varint_u64()
    if len_u64 > 0x7FFF_FFFFUL {
      raise DecodeError("serde_columnar: column too large")
    }
    let len = len_u64.to_int()
    if len < 0 || len > r.remaining() {
      raise DecodeError("serde_columnar: invalid column length")
    }
    cols.push(r.read_exact(len))
  }
  (cols, r.remaining_view())
}

pub fn decode_columnar_vec(bytes: BytesView) -> Array[BytesView] raise DecodeError {
  let (cols, rest) = take_columnar_vec(bytes)
  if rest.length() != 0 {
    raise DecodeError("serde_columnar: trailing bytes")
  }
  cols
}

// Common pattern: serde_columnar::to_vec(&Struct { field: Vec<Row> }) where
// the struct has exactly one field, and that field is annotated with `#[columnar(class="vec")]`.
// In this case the serialized bytes are: varint(1) + (the columnar vec bytes).
pub fn decode_columnar_vec_maybe_wrapped(bytes: BytesView) -> Array[BytesView] raise DecodeError {
  if bytes.length() == 0 {
    return []
  }
  let r = BytesReader::from_view(bytes)
  let first = r.read_varint_u64()
  if first == 1UL {
    return decode_columnar_vec(r.remaining_view())
  }
  decode_columnar_vec(bytes)
}

pub fn encode_columnar_vec_wrapped(cols: Array[Bytes]) -> Bytes {
  let inner = encode_columnar_vec(cols)
  let w = BytesWriter::new()
  w.write_varint_u64(1UL)
  w.write_bytes(inner)
  w.to_bytes()
}

pub fn encode_columnar_vec(cols: Array[Bytes]) -> Bytes {
  let w = BytesWriter::new()
  w.write_varint_u64(cols.length().to_uint64())
  for col in cols {
    w.write_varint_u64(col.length().to_uint64())
    w.write_bytes(col)
  }
  w.to_bytes()
}
