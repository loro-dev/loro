///|
fn count_utf8_codepoints(s : String) -> Int {
  let bytes = @encoding/utf8.encode(s[:])
  let mut count = 0
  for b in bytes {
    // Count non-continuation bytes: 0b10xxxxxx
    let u = b.to_uint()
    if (u & 0xC0) != 0x80 {
      count = count + 1
    }
  }
  count
}

///|
type EncodedMark = (UInt64, CommonValue, Byte) // (key_idx, value, info)

///|
fn take_postcard_vec_marks(
  bytes : BytesView,
) -> (Array[EncodedMark], BytesView) raise DecodeError {
  let r = BytesReader::from_view(bytes)
  let n_u64 = r.read_varint_u64()
  if n_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("richtext: too many marks")
  }
  let n = n_u64.to_int()
  let out : Array[EncodedMark] = []
  for _i in 0..<n {
    let key_idx = r.read_varint_u64()
    let (value, rest) = postcard_take_common_value(r.remaining_view())
    r.skip(r.remaining() - rest.length())
    let info = r.read_u8()
    out.push((key_idx, value, info))
  }
  (out, r.remaining_view())
}

///|
fn encode_postcard_vec_marks(marks : Array[EncodedMark]) -> Bytes {
  let w = BytesWriter::new()
  w.write_varint_u64(marks.length().to_uint64())
  for m in marks {
    let (key_idx, value, info) = m
    w.write_varint_u64(key_idx)
    w.write_bytes(postcard_encode_common_value(value))
    w.write_u8(info)
  }
  w.to_bytes()
}

///|
fn encode_encoded_text(
  span_peer_idx : Array[UInt64],
  span_counter : Array[Int],
  span_lamport_sub : Array[Int],
  span_len : Array[Int],
  keys : Array[String],
  marks : Array[EncodedMark],
) -> Bytes {
  let span_cols = encode_columnar_vec([
    encode_delta_rle_usize(span_peer_idx),
    encode_delta_rle_i32(span_counter),
    encode_delta_rle_i32(span_lamport_sub),
    encode_delta_rle_i32(span_len),
  ])
  let w = BytesWriter::new()
  w.write_varint_u64(3UL)
  w.write_bytes(span_cols)
  w.write_bytes(postcard_encode_vec_string(keys))
  w.write_bytes(encode_postcard_vec_marks(marks))
  w.to_bytes()
}

///|
pub fn transcode_richtext_state_snapshot(
  bytes : BytesView,
  validate : Bool,
) -> Bytes raise DecodeError {
  let (text, rest1) = postcard_take_string(bytes)
  let (peers, rest2) = take_peer_table(rest1)
  let r = BytesReader::from_view(rest2)
  let n_fields = r.read_varint_u64()
  if n_fields != 3UL {
    raise DecodeError("richtext: invalid EncodedText field count")
  }
  let spans_view = r.remaining_view()
  let (span_cols, rest_after_span) = take_columnar_vec(spans_view)
  r.skip(spans_view.length() - rest_after_span.length())
  if span_cols.length() != 4 {
    raise DecodeError("richtext: invalid span column count")
  }
  let span_peer_idx = decode_delta_rle_usize(span_cols[0])
  let span_counter = decode_delta_rle_i32(span_cols[1])
  let span_lamport_sub = decode_delta_rle_i32(span_cols[2])
  let span_len = decode_delta_rle_i32(span_cols[3])
  let span_n = span_len.length()
  if span_peer_idx.length() != span_n ||
    span_counter.length() != span_n ||
    span_lamport_sub.length() != span_n {
    raise DecodeError("richtext: span column length mismatch")
  }
  let keys_view = r.remaining_view()
  let (keys, rest_after_keys) = postcard_take_vec_string(keys_view)
  r.skip(keys_view.length() - rest_after_keys.length())
  let marks_view = r.remaining_view()
  let (marks, rest_after_marks) = take_postcard_vec_marks(marks_view)
  r.skip(marks_view.length() - rest_after_marks.length())
  if r.remaining() != 0 {
    raise DecodeError("richtext: trailing bytes")
  }
  if validate {
    // marks count must match number of span.len == 0
    let mut mark_needed = 0
    let mut text_len_sum = 0
    for i in 0..<span_n {
      let len = span_len[i]
      if len == 0 {
        mark_needed = mark_needed + 1
      } else if len > 0 {
        text_len_sum = text_len_sum + len
      }
      let p = span_peer_idx[i]
      if p > 0x7FFF_FFFFUL || p.to_int() < 0 || p.to_int() >= peers.length() {
        raise DecodeError("richtext: peer_idx out of range")
      }
    }
    if marks.length() != mark_needed {
      raise DecodeError("richtext: mark count mismatch")
    }
    for m in marks {
      let (key_idx, _v, _info) = m
      if key_idx > 0x7FFF_FFFFUL ||
        key_idx.to_int() < 0 ||
        key_idx.to_int() >= keys.length() {
        raise DecodeError("richtext: key_idx out of range")
      }
    }
    if count_utf8_codepoints(text) != text_len_sum {
      raise DecodeError("richtext: text unicode length mismatch")
    }
  }
  let w = BytesWriter::new()
  w.write_bytes(postcard_encode_string(text))
  w.write_bytes(encode_peer_table(peers))
  w.write_bytes(
    encode_encoded_text(
      span_peer_idx, span_counter, span_lamport_sub, span_len, keys, marks,
    ),
  )
  w.to_bytes()
}
