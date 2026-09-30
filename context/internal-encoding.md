# Internal Encoding Context

Verified against code 2026-09-28.

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

Default snapshot export does not walk the alive-container graph. It relies on a
write-time invariant: every container brought alive by an applied op or diff
gets a store entry when the reference is applied
(`DocState::ensure_containers_created_by_op` for local ops,
`DocState::ensure_containers_created_by_internal_diff` for imported/checkout
diffs — this covers empty children that have no ops of their own and therefore
no diff, plus tree `Create` meta maps). Full export is then `flush` + KV export:
byte-faithful, never decodes lazy wrappers, and never materializes state, so a
malformed imported entry round-trips instead of erroring (validation happens
where the wrapper is actually read). Ensured-but-empty mergeable children are
deliberately NOT materialized — the parent map's marker is their single source
of truth and `has_container` must stop resolving them when the marker is
removed (`loro_get_container_for_deleted_mergeable_children`).

Shallow snapshot export still needs the complete alive set for its
`retain_keys` filter. That set is a *retention* set (`AliveWalk::Retention` in
`state.rs`): for Tree containers it follows the meta map of every node,
including deleted ones, because a retained op can revive a node deleted before
the root with its old meta — a node dead only through a deleted ancestor can be
moved out locally, and any peer's `Move` op revives even a directly deleted node
(the handler refuses that locally; undo/`revert_to` create a new `TreeID`
instead, but the CRDT applies it). Map/list children deleted before the root
are still dropped: re-inserting creates a new container id, so no retained op
can re-attach them. When the export also ships a latest-state overlay,
`retain_created_after_root` adds every stored container whose creation id the
root version vector does not include (`!root_vv.includes_id(..)`), never by
matching the root frontiers (that kept almost every container). So "deleted
before the root" is safe to drop only for non-tree children; see
`crates/loro/tests/shallow_snapshot_deleted_containers.rs`, which also checks
checkout into the retained range against a full-history replica.

The cached-root reuse branch (a shallow doc re-exported at its own root, which
also serves `ExportMode::Snapshot` on a shallow doc) applies the same rule. The
cached keys cannot be trusted as the retention set: exporters before #1119 kept
containers deleted before the root whenever they shipped an overlay, and a
re-export was the only way to scrub them. `prune_cached_root` runs on a scratch
doc in two steps:

1. `DocState::unreached_stored_containers(root_vv)` is a cheap filter that
   returns candidate keys. It reads each stored container's parent from its
   encoded header, and it decodes only the containers that are the header
   parent of another stored container. Leaf maps, texts and tree metas are
   never decoded; a tree that parents stored metas is decoded for its node
   list. Two kinds of entries are never candidates. The first is containers
   created after the root (`!root_vv.includes_id(..)`): a root state written
   by the checkout/overlay path keeps an empty placeholder for them that
   nothing at the root references yet. The second is stored children of a
   *reached*, stored container of an unknown type: unknown states decode to
   `Null`, so no walk can list their children. An unknown id that appears
   only as a header parent, or an unreferenced unknown entry, protects
   nothing. With no candidates, the root is reused as-is. That is the case
   for roots written by current exporters with only valid, reachable
   entries. Debug builds assert that the full walk agrees when no
   unknown-type container is stored.
2. With candidates, the full `ensure_all_alive_containers` walk runs as well.
   A container counts as reached in step 1 only through its header parent, so
   a forged header makes it a candidate; the full walk then rejects the
   inconsistency with `Err` instead of dropping a container that is still
   referenced. `export_shallow_snapshot_inner` logs that `Err` and exports the
   root verbatim, as exporters before #1123 did: a root that cannot be judged
   (documents hit by the #1161 family can hold one) must still be exportable,
   and keeping everything drops nothing that is referenced. Only keys that
   both steps leave unreached are removed. The
   filter protects placeholders and unknown subtrees that the walk cannot see,
   and the walk vetoes anything it still reaches. Never drop keys on either
   step's word alone.

The overlay (>256 ops) branch still rejects unknown root keys, as before.

The result (`None` for "reuse as-is", including the verbatim fallback, or the
pruned bytes plus the removed keys) is memoized in
`GcStore::pruned_root` because the cached root never changes. Only the first
re-export of a root pays for the check. Repeated re-exports cost the same as
before the fix, legacy roots are not re-encoded every time, and import is
unaffected. `LoroDoc::fork` uses `encode_snapshot_inner_for_fork`
(`CachedShallowRoot::Verbatim`): a fork copies the cached root verbatim, so it
never runs the check or panics on an inconsistent root. Do not replace the
check with the latest state's alive set: tree metas that are dead at the latest
version but alive inside the retained range would be lost.
`legacy_*.bin` fixtures in the same test file pin both the dead-map drop and
tree-meta revival for such blobs. The forged-header and unknown-container
regressions are `cached_root_*` unit tests in `shallow_snapshot.rs`.
`crates/loro/tests/perf_shallow_reexport.rs` is the ignored release benchmark
for first and repeated re-export and import.

The pure TypeScript runtime (`loro-js`) uses the same retention rule when it
rebuilds both states in `LoroDoc.#encodeShallowSnapshot`. The root state keeps
`#retainedContainerKeys()` at the root: root containers (every mergeable
container is one, as in Rust's `existing_retention_roots`, so a child hidden by
a deleted or different-kind marker keeps its state), visible Map/List children,
and every tree node's meta, including deleted nodes. The latest state
additionally keeps containers alive at the latest version and containers whose
creation id the root version does not include. `loro-js` always rebuilds the
root state by replay instead of reusing its cached root store, so re-exporting
an older blob at the same root also prunes it (the #1123 case). Tests are in
`loro-js/tests/shallow-snapshot-deleted-containers.test.ts`.

Two import-side pieces support revived tree nodes. `TreeOpGroup::record_shallow_root_state`
seeds the tree diff cache with deleted nodes as well (directly deleted as
`Delete`, their descendants as `Create` under their real parent); with only
alive nodes, reviving a node lost its root-time subtree and checkout could not
retreat later moves of its children. `DocState::register_meta_parents_of_created_tree_nodes`
registers the tree as the parent of each (re)created node's meta before a diff
batch is sorted by depth, so blobs from pre-fix exporters (root state without
the revived meta) no longer panic with "Parent is not registered" or drop the
meta's diff as a dangling container.

The alive walk in `DocState::ensure_all_alive_containers`
registers root keys, reads snapshot-backed values ephemerally (via
`try_get_value_ephemeral`, which never caches the decoded value or retains a
probe-only wrapper), and only inserts a wrapper when an alive container has no
KV entry. Do not replace this with `InnerStore::load_all` or cache every
decoded value: documents with many small/deleted containers retain that memory
for the rest of the WASM instance. The walk may retain only the resulting set
of arena indices, capped at an estimated 4 MiB, reused while both the state
frontiers and the existing retention-root list are unchanged (an empty
top-level root does not advance the CRDT version), bypassed during a
transaction. The walk reads an uncached container's encoded parent and value
through one temporary wrapper, checks the parent header against the reachable
edge (or a mergeable ID's intrinsic edge), and returns an export error on
conflicting external state. Decompressed SSTable blocks are bounded separately
by the kv-store's byte-weighted block cache (`BLOCK_CACHE_MAX_BYTES` in
`sstable.rs`).

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

The general diff path may choose a replay base older than the current state
(the latest single-head critical version, Eg-walker §3.5) so list-like trackers
have enough position context. When that happens,
`DiffCalculator::calc_diff_internal` still walks the common causal history, but
routes it only to containers that have operations in the version-vector
difference between `before` and `after`. Do not treat every container seen since
the conservative base as changed: the List/Text/MovableList safety fallback can
otherwise replay the full history once per unchanged container.

The replay-base walk expands both explicit change dependencies and the implicit previous
counter of the same peer. A change from an existing peer can therefore produce
two paths: an explicit relay dependency and an implicit same-peer predecessor.
The relay may already contain that predecessor. In that case the second path can
reach the end of the queue without meeting the other side even though it is not
concurrent.

To distinguish those cases, `_find_meet_and_mode` carries the dependency
tip where each path split. When a path remains unmatched, it checks only that tip
against the ancestors of the candidate common frontiers. A covered tip is a
redundant route and does not lower the replay base; an uncovered tip is a real
concurrent branch and keeps the conservative fallback. This is a targeted DAG
reachability check with visited-node and Lamport pruning. Do not replace it with
a complete version-vector containment check for every new peer range: that work
scales with both the update's peer count and the size of the current version
vector, and it duplicates the causal decision the walk is already making.

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

`SnapshotAt` MUST retain the encoded state of normal containers whose creating
operation belongs to the exported version, including deleted descendants.
Historical checkout reverses operations on those containers; an alive-only
filter can leave their operation history present while their materialized state
is absent. `prepare_snapshot_container_state` owns construction of the filtered
state copy. It combines `alive_indices_to_bytes` with
`retain_containers_at_version`, which extends the alive set from encoded KV
keys and checks creation IDs against the target version vector. It excludes normal
containers created after the target, without decoding their state. Root and
mergeable-container retention follows the existing alive walk. Shallow and
state-only exports keep their history-boundary filtering and redaction rules.
The public `snapshot_at_test` regressions exercise historical diffs, removed
list/text descendants, later-created containers, and detached-source restoration.
All recoverable errors after the temporary checkout MUST reach source
restoration. The saved attachment mode MUST be restored explicitly, including
a source detached at its current head.

For `ShallowSnapshot`, the root is the latest single-head *critical version*
(`latest_single_head_critical_version`, spec lemma L11) of the requested
frontier together with the latest version, because every op from the root up
to the latest version is retained and each one must be causally before or
after the root, never concurrent with it (loro-dev/loro#1095). The meet of
the requested heads is not enough: a branch merged later that
forked below the requested version is concurrent with it. `StateOnly` retains
history only up to its target, so it uses the target on both sides.
`crates/loro/tests/shallow_root_critical.rs` checks the property op by op.
Because the root state is materialized by a checkout, it is only as correct as
the checkout's winner metadata (lamport/peer), not just its values: see
"Winner metadata, not just values" in
[crates/loro-internal/docs/diff_calc.md](../crates/loro-internal/docs/diff_calc.md). The
root is then moved past a rich-text StyleStart when necessary and clamped to
an existing shallow root. The root state carries `fr`; a later state overlay
does not. Import loads the root first and then either overlays the later state
or replays retained changes when the state section is `E`. Unknown handling is
path-dependent: rebuilding a root, or reusing a cached root to build an
overlay, rejects unknown root-state containers that survive retention
filtering; the cached-root replay-only `E` fast path skips that check. Containers introduced after the
root are not checked again and can survive either in retained operations (`E`)
or as raw/lazy overlay state bytes.

When the source doc is not shallow, its state is already at the latest
version, and at least `MIN_RETAINED_OPS_FOR_FORWARD_ROOT_STATE` (65536; 16 in
unit tests) ops are retained since the root, `export_shallow_snapshot_inner`
builds the root state by replaying pre-root history forward into a temporary
doc (`export_fast_updates_in_range` pre-encoded under the oplog lock, then
imported), not by checking the live doc out backwards: a reverse checkout
makes the richtext/list diff calculators rebuild a full CRDT tracker from
empty per touched container (the `should_rebuild` path in
`RichtextDiffCalculator::calculate_diff`), which dominated shallow export cost
(~20x slower than forward replay on container-heavy docs). Below the retained-ops
threshold the checkout path is used instead: it ties in time around ~8k
retained ops and peaks at ~4x less memory, which matters for lazily imported
docs (exporting right after import must not materialize the whole state).
The fast path pays for re-encoding and replaying the ENTIRE pre-root history,
so the prefix is also gated: `pre_root_ops <= 16 * ops_num`
(`MAX_PRE_ROOT_TO_RETAINED_OPS_RATIO`; measured crossover — fast wins at
ratio 9, loses at 19), `pre_root_ops <= 1_000_000`
(`MAX_PRE_ROOT_OPS_FOR_FORWARD_REPLAY`), and a decoded-byte cap on the prefix
(`MAX_PRE_ROOT_BYTES_FOR_FORWARD_REPLAY`, 32 MiB) because op counts miss value
sizes — a Map write is one atom regardless of how large its Binary/String
value is. The byte leg runs BEFORE encoding: `estimate_ops_content_bytes`
walks op payloads by reference (arena slices are never copied), recursing into
nested `LoroValue::List`/`Map` and counting everything the block encoder
copies: map and style keys, style values, fractional indexes, root container
names, unknown-op OwnedValue payloads (including MarkStart keys and
MarkStart/ListSet values), and commit messages, with a budget-aware early
exit past the cap, while
`export_fast_updates_in_range` slice-copies values into a fresh store — so the
cap must be checked before any prefix bytes are copied.
A huge unrelated prefix with a large tail must stay on the checkout path —
see the `shallow_export_scalar_prefix` and `shallow_export_byte_prefix`
benches.
The replay doc mirrors the live store's root container entries via
`DocState::existing_retention_roots` (a root-only key scan — never
`iter_all_container_ids`, which calls `load_all`) so accessed-but-op-less root
containers still ship, and it receives a copy of the live doc's
`deleted_root_containers` config so roots deleted before the root are dropped
at flush instead of being resurrected as empty entries (roots deleted after
the root keep their at-root content because flush only drops entries whose
value is empty). Detached or already-shallow sources keep the old checkout
path (the reuse branch handles cached roots; a shallow source's trimmed
history cannot be forward-replayed). The forward path never moves the live
doc, so no state restore is needed.

Pre-shallow frontier safety lives in `loro.rs`: `checkout`, `diff`, and
`revert_to` must return `SwitchToVersionBeforeShallowRoot` instead of traversing
history before the shallow root.

Merge semantics of a shallow replica meeting concurrent full-history peers
(which updates apply, pend, or are rejected, and why a never-synced peer can
never merge): [docs/shallow-snapshot-concurrency.md](../docs/shallow-snapshot-concurrency.md)
with tests in `crates/loro/tests/shallow_snapshot_concurrency.rs`.

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
