pub struct ChangesMeta {
  timestamps: Array[Int64]
  commit_msgs: Array[String?]
}

pub fn ChangesMeta::timestamps(self: ChangesMeta) -> Array[Int64] {
  self.timestamps
}

pub fn ChangesMeta::commit_msgs(self: ChangesMeta) -> Array[String?] {
  self.commit_msgs
}

pub fn decode_changes_meta(bytes: BytesView, n_changes: UInt) -> ChangesMeta raise DecodeError {
  let n_u64 = n_changes.to_uint64()
  if n_u64 > 0x7FFF_FFFFUL {
    raise DecodeError("change_meta: too many changes")
  }
  let n = n_u64.to_int()
  if n <= 0 {
    raise DecodeError("change_meta: empty block")
  }

  let (timestamps, rest1) = delta_of_delta_take_n_finalize_i64(bytes, n)
  let (lens_u32, rest2) = any_rle_take_n_finalize_u32(rest1, n)
  let mut total: Int = 0
  let lens: Array[Int] = []
  for x in lens_u32 {
    let xi = x.reinterpret_as_int()
    if xi < 0 {
      raise DecodeError("change_meta: invalid msg len")
    }
    total = total + xi
    lens.push(xi)
  }
  if total < 0 || total > rest2.length() {
    raise DecodeError("change_meta: invalid msg bytes length")
  }

  let msgs: Array[String?] = []
  let mut offset = 0
  for len in lens {
    if len == 0 {
      msgs.push(None)
      continue
    }
    let end = offset + len
    if end < 0 || end > rest2.length() {
      raise DecodeError("change_meta: msg bytes overflow")
    }
    let s = @encoding/utf8.decode(rest2[offset:end]) catch { @encoding/utf8.Malformed(_) =>
      raise DecodeError("change_meta: invalid utf8 msg")
    }
    msgs.push(Some(s))
    offset = end
  }
  if offset != total {
    raise DecodeError("change_meta: msg bytes trailing")
  }
  { timestamps, commit_msgs: msgs }
}
