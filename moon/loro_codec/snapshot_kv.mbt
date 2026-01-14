const DEFAULT_SSTABLE_BLOCK_SIZE: Int = 4096

fn bytes_eq(a: Bytes, b: Bytes) -> Bool {
  if a.length() != b.length() {
    return false
  }
  for i in 0..<a.length() {
    if a[i] != b[i] {
      return false
    }
  }
  true
}

pub fn transcode_oplog_kv_store(oplog_bytes: Bytes, validate: Bool) -> Bytes raise DecodeError {
  let kvs = sstable_import_all(oplog_bytes, validate)

  if validate {
    let mut seen_vv = false
    let mut seen_fr = false
    let mut seen_sv = false
    let mut seen_sf = false
    for kv in kvs {
      let (k, v) = kv
      if bytes_eq(k, b"vv") {
        if seen_vv {
          raise DecodeError("oplog: duplicate vv")
        }
        let (_vv, rest) = postcard_take_version_vector(v[:])
        if rest.length() != 0 {
          raise DecodeError("oplog: vv trailing bytes")
        }
        seen_vv = true
      } else if bytes_eq(k, b"fr") {
        if seen_fr {
          raise DecodeError("oplog: duplicate fr")
        }
        let (_fr, rest) = postcard_take_frontiers(v[:])
        if rest.length() != 0 {
          raise DecodeError("oplog: fr trailing bytes")
        }
        seen_fr = true
      } else if bytes_eq(k, b"sv") {
        if seen_sv {
          raise DecodeError("oplog: duplicate sv")
        }
        let (_sv, rest) = postcard_take_version_vector(v[:])
        if rest.length() != 0 {
          raise DecodeError("oplog: sv trailing bytes")
        }
        seen_sv = true
      } else if bytes_eq(k, b"sf") {
        if seen_sf {
          raise DecodeError("oplog: duplicate sf")
        }
        let (_sf, rest) = postcard_take_frontiers(v[:])
        if rest.length() != 0 {
          raise DecodeError("oplog: sf trailing bytes")
        }
        seen_sf = true
      } else {
        if k.length() != 12 {
          raise DecodeError("oplog: invalid key length")
        }
        let block_id = id_from_change_block_key(k[:])
        let changes = decode_change_block(v[:])
        if changes.length() == 0 {
          raise DecodeError("oplog: empty change block")
        }
        if changes[0].id() != block_id {
          raise DecodeError("oplog: block key mismatch")
        }
      }
    }

    // Not all snapshot kinds include shallow keys, but vv/fr should always exist.
    if !seen_vv || !seen_fr {
      raise DecodeError("oplog: missing vv/fr")
    }
  }

  // Re-encode SSTable (compression=None) for canonical Moon output.
  try {
    sstable_export_all(kvs, DEFAULT_SSTABLE_BLOCK_SIZE)
  } catch {
    EncodeError(msg) => raise DecodeError("oplog: encode sstable failed: " + msg)
  }
}

pub fn transcode_state_kv_store(state_bytes: Bytes, validate: Bool) -> Bytes raise DecodeError {
  // For shallow_root_state_bytes in non-shallow snapshots.
  if state_bytes.length() == 0 {
    return state_bytes
  }
  // Special sentinel for empty state.
  if state_bytes.length() == 1 && state_bytes[0] == b'E' {
    return state_bytes
  }

  let kvs = sstable_import_all(state_bytes, validate)
  if kvs.length() == 0 {
    return b"E"
  }

  if validate {
    for kv in kvs {
      let (k, v) = kv
      if bytes_eq(k, b"fr") {
        let (_fr, rest) = postcard_take_frontiers(v[:])
        if rest.length() != 0 {
          raise DecodeError("state: fr trailing bytes")
        }
        continue
      }

      let cid = container_id_from_bytes(k[:])
      let wrapper = parse_container_wrapper(v[:])
      // Basic consistency check: wrapper kind should match key's container kind.
      match cid {
        ContainerID::Root(_, kind) | ContainerID::Normal(_, _, kind) => {
          if wrapper.kind() != kind {
            raise DecodeError("state: container kind mismatch")
          }
        }
      }
    }
  }

  // For now, keep payload opaque and only normalize the wrapper prefix.
  let out_kvs: Array[(Bytes, Bytes)] = []
  for kv in kvs {
    let (k, v) = kv
    if bytes_eq(k, b"fr") {
      out_kvs.push((k, v))
      continue
    }
    let wrapper = parse_container_wrapper(v[:])
    let payload = transcode_container_state_snapshot(wrapper.kind(), wrapper.payload_view(), validate)
    let out_v = encode_container_wrapper(
      wrapper.kind(),
      wrapper.depth(),
      wrapper.parent(),
      payload,
    )
    out_kvs.push((k, out_v))
  }

  try {
    sstable_export_all(out_kvs, DEFAULT_SSTABLE_BLOCK_SIZE)
  } catch {
    EncodeError(msg) => raise DecodeError("state: encode sstable failed: " + msg)
  }
}
