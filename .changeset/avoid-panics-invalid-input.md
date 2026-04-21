---
"loro-crdt": patch
"loro-crdt-map": patch
---

Harden encoding, snapshot, and import paths against malformed input

- JSON schema import (`import_json_updates`): out-of-range compressed peer indices now return `DecodeError` instead of being silently accepted as raw peer IDs; mismatched `JsonOpContent` vs container type returns `DecodeError` instead of panicking.
- Outdated binary encoding decoder (`decode_op`): malformed op streams (missing delete iterators, type mismatches) now return `DecodeDataCorruptionError` instead of panicking.
- Fast snapshot decoder (`decode_snapshot_blob_meta`): truncated or oversized section lengths now return `DecodeDataCorruptionError` instead of panicking on slice indexing.
- Change store KV import (`import_all`): corrupted `VersionVector`/`Frontiers` metadata now returns `DecodeDataCorruptionError` instead of panicking.
- Value encoding (`LoroValueKind::from_u8`, `read_str`): invalid byte values and invalid UTF-8 now return `DecodeDataCorruptionError` instead of panicking.
- `LoroDoc::diff()`: checkout failures during diff calculation are now propagated as `LoroError` instead of panicking; state restore uses `unwrap()` to fail-fast on internal errors.
- `try_get_text/list/map/tree/movable_list/counter`: now return `None` for wrong root container types instead of panicking.
- Detached list insert out-of-bounds: returns `LoroError::OutOfBound` instead of panicking.
- Tree `mov_after`/`mov_before` on deleted node: returns `TreeNodeDeletedOrNotExist` instead of panicking.
- `JsonChange::op_len`: empty ops array returns `0` instead of panicking.
- `renew_peer_id`: avoids theoretical collision with `PeerID::MAX`.
