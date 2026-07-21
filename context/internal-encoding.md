# Internal Encoding Context

Verified against code 2026-07-20.

Loro has one binary blob envelope, two current binary body formats, two
recognized-but-unsupported legacy top-level modes, and a separate JSON updates
schema. The most common mistake is to treat `outdated_encode_reordered.rs` as an
obsolete file; only top-level blob modes 1 and 2 are obsolete. Several helpers in
that file are still used by current fast paths.

## Two-Hop Answer

If an agent asks "how does Loro encoding work?", start here:

- [crates/loro-internal/src/encoding.rs](../crates/loro-internal/src/encoding.rs):
  `ExportMode`, `EncodeMode`, `parse_header_and_body`, `encode_with`,
  `decode_oplog_changes`, `decode_snapshot`, `decode_import_blob_meta`.
- [crates/loro-internal/src/loro.rs](../crates/loro-internal/src/loro.rs):
  `LoroDoc::_import_with` chooses snapshot-vs-updates application behavior.
- [crates/loro-internal/src/encoding/fast_snapshot.rs](../crates/loro-internal/src/encoding/fast_snapshot.rs):
  `Snapshot`, `encode_snapshot_inner`, `decode_snapshot_inner`, `encode_updates`,
  `decode_updates`.
- [crates/loro-internal/src/encoding/shallow_snapshot.rs](../crates/loro-internal/src/encoding/shallow_snapshot.rs):
  `export_shallow_snapshot_inner`, `export_state_only_snapshot`,
  `encode_snapshot_at`.
- [crates/loro-internal/src/encoding/json_schema.rs](../crates/loro-internal/src/encoding/json_schema.rs):
  `JsonSchema`, `export_json`, `decode_changes`, `redact`.
- [docs/encoding.md](../docs/encoding.md),
  [docs/encoding-container-states.md](../docs/encoding-container-states.md),
  [docs/encoding-lz4.md](../docs/encoding-lz4.md), and
  [docs/encoding-xxhash32.md](../docs/encoding-xxhash32.md): normative current
  binary-format references, pinned to a verified code commit.

## Binary Envelope

Every binary export starts with:

- magic bytes `loro` from `encoding.rs:MAGIC_BYTES`,
- a 16-byte checksum field,
- a big-endian `u16` `EncodeMode`,
- mode-specific body bytes.

For current `FastSnapshot` and `FastUpdates` blobs, `ParsedHeaderAndBody::check_checksum`
uses `xxhash32` over bytes starting at offset 20, which includes the mode bytes
and body. Legacy modes use the older MD5 check path only for detection.

## Supported And Outdated Modes

Current modes:

- `EncodeMode::FastSnapshot = 3`: used by `ExportMode::Snapshot`,
  `ShallowSnapshot`, `StateOnly`, and `SnapshotAt`.
- `EncodeMode::FastUpdates = 4`: used by `ExportMode::Updates` and
  `UpdatesInRange`.

Recognized but unsupported top-level modes:

- `EncodeMode::OutdatedRle = 1`
- `EncodeMode::OutdatedSnapshot = 2`

`encoding.rs:decode_oplog_changes`, `encoding.rs:decode_snapshot`, and
`LoroDoc::decode_import_blob_meta` return `ImportUnsupportedEncodingMode` for
these outdated top-level modes. Do not extend them without compatibility
fixtures and a migration plan.

Important nuance: [outdated_encode_reordered.rs](../crates/loro-internal/src/encoding/outdated_encode_reordered.rs)
still contains current helpers including `import_changes_to_oplog`, `encode_op`,
`decode_op`, and `ValueRegister`.

## FastSnapshot

`fast_snapshot.rs:Snapshot` has three body sections:

1. `oplog_bytes`: KV-store encoded change history.
2. `state_bytes`: KV-store encoded materialized state, or the one-byte
   `EMPTY_MARK` (`E`) sentinel when a shallow snapshot omits its end-state
   overlay and retained history must be replayed. A zero-byte state section is
   an empty KV store, not `EMPTY_MARK`.
3. `shallow_root_state_bytes`: KV-store encoded shallow root state; empty for a
   non-shallow snapshot.

`decode_snapshot_inner` only initializes directly when importing into an empty
document. If a snapshot is imported into a non-empty document,
`LoroDoc::_import_with` routes through decoded oplog changes instead. Failed
direct snapshot import must reset both state and oplog.

Default snapshot export must still persist alive empty child containers, but it
must not materialize the full lazy state store to find them. The alive walk in
`DocState::get_all_alive_containers` registers root keys, reads snapshot-backed
values ephemerally (via `try_get_value_ephemeral`, which never caches the
decoded value or retains a probe-only wrapper), and only inserts a wrapper when
an alive container has no KV entry. Shallow snapshot retention reuses the same
complete alive set of arena indices. Do not replace this with
`InnerStore::load_all` or cache every decoded value: documents with many
small/deleted containers retain that memory for the rest of the WASM instance.
`DocState::ensure_all_alive_containers` may retain only the resulting set of
arena indices after a successful walk, capped at an estimated 4 MiB. It reuses
that set while both the state frontiers and the existing retention-root list
are unchanged, bypasses the cache during a transaction, and drops an obsolete
set before constructing its replacement. The root-list part is required
because obtaining a new empty top-level root does not advance the CRDT version.
The walk also reads an uncached container's encoded parent and value through one
temporary wrapper, so a single export does not probe the same lazy KV entry
twice. Neither cache retains decoded container values.
Decompressed SSTable blocks are bounded separately by the kv-store's
byte-weighted block cache (`BLOCK_CACHE_MAX_BYTES` in `sstable.rs`), so the walk
uses plain cached reads. The walk reads each present wrapper's parent header
and checks it against the reachable edge (or a mergeable ID's intrinsic edge).
Conflicting external state must return an export error; an alive empty child
with no wrapper may inherit the edge that made it reachable.

`Snapshot::encoded_len` is available after the three section `Bytes` values are
created. The default exporter uses it to reserve the final envelope once; keep
the checksum offset and the `EMPTY_MARK` length in that calculation aligned with
`_encode_snapshot`.

## FastUpdates

`FastUpdates` is a sequence of LEB128 length-prefixed change blocks.
`fast_snapshot.rs:decode_updates` rejects invalid block lengths, length
overflow, and truncated block payloads, then sorts decoded changes by lamport.
`encoding.rs:apply_decoded_changes_to_oplog` imports changes, separates pending
changes, applies newly-unlocked pending changes, and rejects dependencies before
a shallow root.

`loro.rs:isolated_scalar_root_batch` is a conservative import optimization for
a causally closed batch of brand-new peers whose operations are scalar Map
writes on top-level root names absent from old history and current state. It is
checked against the decoded changes *before* they are applied to the oplog,
while the store still holds only old history: every batch peer must be absent
from the current vv and contiguously covered from counter 0, every dependency
must point inside the batch, and every op must be a scalar Map write on a root
Map (no container values, no mergeable markers). After apply, the candidate is
kept only if the vv delta equals exactly the batch spans (pending changes may
have been unlocked). The fast path then calculates the state diff from the
empty version to that batch, avoiding replay of an unrelated large history.
`ChangeStore::old_history_may_touch_root_names` answers the "absent from old
history" part with a size-capped set of every root name in the store, built
once by scanning encoded block container arenas (no op parsing, no
parsed-change cache fill) and updated incrementally on each inserted change.
Because the set is only ever conservative, a rollback needs no invalidation —
stale names just force the general diff path. Decode failures or exceeding the
name-byte cap permanently disable the optimization for that store.

For a large snapshot regression check, first build the Node package, then run:

```sh
pnpm -C crates/loro-wasm test:snapshot-memory -- /path/to/input.procloud 100
```

The budget argument is in MiB. Use `95` when the requirement is a strict
100,000,000-byte ceiling; 95 MiB is about 99.6 MB.

The gate keeps the `toJSON()` result alive through update import and snapshot
export. It measures the memory directly attributable to this API round: input
snapshot/update buffers, the WASM linear-memory high-water mark, the returned
snapshot `Uint8Array`, retained JS heap growth, and unique binary backing stores
reachable from the retained JSON value. It reports process RSS separately; RSS
also includes the Node/V8 runtime and executable/code pages.

## Shallow, State-Only, And SnapshotAt

All three use `FastSnapshot` mode; there is no on-wire subtype field:

- `ShallowSnapshot` retains history since a calculated shallow start frontier.
- `StateOnly` is a shallow snapshot with minimal history at the target version.
- `SnapshotAt` exports full history up to target frontiers plus state at that
  version, but only from a non-shallow source document; a shallow source
  currently returns `NotImplemented`.

For `ShallowSnapshot`, the requested frontier is reduced to a single valid
history boundary, moved past a rich-text StyleStart when necessary, and clamped
to an existing shallow root. The root state carries `fr`; a later state overlay
does not. Import loads the root first and then either overlays the later state
or replays retained changes when the state section is `E`. Unknown handling is
path-dependent: rebuilding a root, or reusing a cached root to build an
overlay, rejects unknown root containers; the cached-root replay-only `E` fast
path reuses the root bytes without that check. Containers introduced after the
root are not checked again and can survive either in retained operations (`E`)
or as raw/lazy overlay state bytes.

Pre-shallow frontier safety lives in `loro.rs`: `checkout`, `diff`, and
`revert_to` must return `SwitchToVersionBeforeShallowRoot` instead of traversing
history before the shallow root.

## JSON Updates

`json_schema.rs` is not wrapped in the binary `loro` envelope. Its
`JsonSchema` carries:

- `schema_version = 1`,
- `start_version`,
- optional peer compression table,
- JSON changes and ops.

Malformed JSON schema should return `Err` without partial import. Look at
[crates/loro-internal/src/tests/import_atomicity.rs](../crates/loro-internal/src/tests/import_atomicity.rs)
when changing JSON import validation or rollback behavior.

## Validation Shortcuts

- Binary malformed input or rollback: `cargo test -p loro-internal import_atomicity`
- Truncated fast updates: `cargo test -p loro-internal decode_updates_rejects_truncated_block`
- Pre-shallow checkout/diff/revert behavior:
  `cargo test -p loro --test issue issue_928` and
  `cargo test -p loro --test contracts shallow`
- Snapshot retention that might involve mergeable containers:
  `cargo test -p loro-internal --test mergeable_container`
- Shared behavior: root `pnpm test`

## Common Misconceptions

- "Outdated modes are still supported because `LoroDoc::_import_with` branches on
  them." They are detected, then route to decode paths that return unsupported.
- "`outdated_encode_reordered.rs` is dead." It is legacy-named but still contains
  active op/value helpers.
- "Snapshot import always initializes state directly." Only empty docs can reset
  from snapshot; non-empty imports use oplog-change application.
