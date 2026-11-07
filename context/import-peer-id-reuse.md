# Imports That Reuse Local Op Ids

Verified against code 2026-11-07.

An import skips the part of each change the doc already has by version vector and
applies the rest on top of the local history. When two clients shared a peer id
(loro-dev/loro#1118), the "known" part is different history. The rest was then
applied to a state it was not written against. It either panicked inside
`RichtextState::apply_diff` while the doc locks were held, poisoning them (a later
`Drop` then aborted the process), or it returned `Ok` with scrambled text. Since
loro-dev/loro#1160, `RichtextState::validate_diff` rejects the out-of-range
case after the fact. The check below rejects both cases before anything changes,
and names the reused id.

## The check

`OpLog::check_and_trim_known_part_of_changes` (`crates/loro-internal/src/oplog/known_history.rs`)
runs in `LoroDoc::import_changes_and_apply_delta_to_state_if_needed` right after
decode, before the preflight and before anything is mutated. That function is the
single entry for binary updates, snapshots imported into a non-empty doc, JSON
updates, and each blob of `import_batch`. An error takes the existing early-exit
path, which rolls back only the arena (decode may have registered containers).

1. **Content.** Every imported change that overlaps the local version vector is
   compared with the local changes over the overlap, also when the import brings
   nothing new. A mismatch returns `LoroError::UsedOpID { id }` with the first
   differing id.
2. **Parked history.** The part of each change the version vector does not cover
   is compared with parked changes that hold the same ids (see below). A mismatch
   returns `UsedOpID` the same way.
3. **The import itself.** Changes of one import are compared with each other over
   that same part (see "One import").
4. **Trim.** The known parts are dropped, as the decoders used to do.
5. **Skipped counters.** A change whose deps are present but whose peer's
   previous counter is not is rejected with `DecodeError`. The main pass of
   `import_changes_to_oplog` would apply it and trip the counter assertion in
   `AppDag::update_version_on_new_change` under the op log lock. The check replays
   that pass on counter ends only. Parked pending changes already wait for their
   own predecessor (`remote_change_apply_state`).

Binary decoding can skip a fully known block whose bytes match the current
local block exactly (`ChangeStore::contains_encoded_block`). A dirty parsed
block shadows the KV copy, so stale bytes never authorize this shortcut.
Different bytes still use the semantic check: encoding, change boundaries,
timestamps, and messages are not required to match.

For snapshots imported into a loaded document,
`ChangeStore::decode_snapshot_for_updates` decodes unmatched blocks in a temporary
arena, checks and trims there, and moves only the retained suffix into the document
arena. Frontier blocks parsed by `import_all` are taken, not cloned. No dropped
prefix strings or list values are allocated in the document arena, including when
snapshot and local history use different change boundaries. The regular import
entry then runs its counter/preflight checks as usual.

## Parked changes

Two writers can reuse an id on top of a dep the receiver lacks, so both updates are
parked. When the dep arrives, one is applied and the other reaches
`ChangeState::Applied` in `try_apply_pending` and disappears, with every import
returning `Ok`. Both contents were received, so unlike delta sync this is comparable
without any format change.

The check runs when the second update arrives, not when the dep does:

- The update that brought the conflict gets the error. At unlock it would land on
  the dep's import, which is innocent.
- Nothing has changed yet, so no rollback is needed. An error at unlock has to undo
  the applied dep, and then the conflict stays parked: the dep can never be imported
  again and sync is stuck for good.
- It gives the same outcome as known history: the first writer stays, and the second
  is rejected. It also covers an update that could apply at once while a conflicting
  one is parked.

`PendingChanges::changes` is keyed by the missing dep, so it cannot say who holds an
id, and scanning it per import makes out-of-order delivery quadratic.
`PendingChanges::claims` maps, per peer, disjoint counter spans to the parked change
that first held them. Comparing with one holder is enough: everything else parked over
those ids was compared with it on arrival. `claim` adds them when a change first parks,
and `release_claims_below` drops what the op log now has.

Claims cannot be rebuilt from `changes`. `JsonLossy` equality is not transitive: null
matches NaN and infinity, which do not match each other, so a doc can park all three
for one id only while null holds it. A rebuild picks a holder by slot order, and then
an update accepted before a failed import is rejected after it. So
`PendingChangesRollback` logs every claim edit and undoes the log in reverse, while it
restores `changes` by slot. `apply_diff` scopes log nothing, as they park nothing.

The same non-transitivity remains when the ids reach the op log. The first of the
parked changes to apply decides the value, and later imports compare with that value:
with null held and NaN and infinity parked, infinity applied by another slot makes a
repeat of NaN a `UsedOpID`. That is a true conflict that null was hiding, and deps are
compared exactly in both modes, so it cannot break the DAG.

## Local writes

A local write takes the next id of its peer. When parked changes hold that id or any
later one, or depend on one, the write forks the history they were written on:

- A held id: the dep applies the rest of the parked change over the write. Its lamport
  was computed from its own deps, so `AppDag` asserts `lamport is not continuous` under
  the op log lock, which poisons it and aborts the process on drop.
- A parked tail past the next id: the writes get there, and the tail is applied onto
  history it does not continue, or stays parked for good.
- An awaited id (`1@0` waits for `7@0`, and the local `7@0` is something else): the
  next import that has peer 7 applies `1@0` on the wrong base. This holds for every dep
  of a parked change, and `changes` slots only the first missing one. With `9@0` on
  `{5@0, 7@0}` a write under the other dep gave the same version vector as the source
  and different values, with no error. So `claim` records the greatest counter each peer
  is depended on in `PendingChanges::awaited`. A change leaves parking only with its
  deps in the op log, so the entry stops mattering by itself.

`check_change_greater_than_last_peer_id` reports all of them from
`Transaction::new_with_origin` through `PendingChanges::last_parked_at_or_after`, with
`ConcurrentOpsWithSamePeerID`, in attached mode too.

### Imports and transactions

The check runs once, when the transaction starts. That is enough only if these hold:

1. **An import and a transaction start exclude each other.** Every import holds the
   doc's transaction lock from start to end, one call or a whole `import_batch` with
   all its blobs (`with_barrier`, `BatchImportGuard`). Every transaction starts under
   that lock: auto-commit ones where the lock is already held (`txn_under_lock`), and
   manual ones (`LoroDoc::txn`, also reachable as `loro::LoroDoc::inner().txn()`) take
   it in `txn_with_origin`.
2. **No import runs while a transaction is open.** The auto-commit one is committed by
   the barrier. A manual one outlives the lock, so each import returns
   `ImportWhenInTxn` before it changes anything: `import_batch` before it detaches or
   opens its rollback scope, the others in `check_not_in_txn` under the op log lock.
   Its ops are in the version vector and not in the change store, so a comparison could
   not see them either.
3. So **parked history does not change between a transaction's start and its commit**,
   and what the start checked still holds at the commit, and **no local change lands in
   an import's rollback scope**, which would undo it in the op log and not the state.
4. **A refused side leaves no lock poisoned.** The barrier still renews the auto-commit
   transaction after a refused import; with a manual one open, `auto_commit_txn` leaves
   none (`DuplicatedTransactionError`), and the next write reports that.

Checking at commit instead would be too late: the ops are in the state by then, and a
commit cannot take them back. `import_local_change` asserts 3 in debug builds.

Before 1 held, a manual transaction could start between two blobs of a batch: its
check missed the later blobs, they applied on top of its write, and a failed batch took
its change out of the op log and left it in the state (loom test
`manual_transaction_during_batch_import`).

Other operations that read history, such as `checkout`, do not check for an open manual
transaction, and its uncommitted ops break them (`DAG node must have a change`). That is
outside imports and not fixed here.

`auto_commit_txn` then leaves
no transaction, and the next write reports the error, the same as in detached editing
(`context/version-input-contract.md`). The write works again once the dep arrives, or
with another peer id.

A remote can use this to stop a doc writing with a peer id, by parking a change under
that id. Peer ids are random and public in updates, and a forged update already could
abort the doc this way, so this is the better failure. The way out is `set_peer_id`.

## One import

Two changes of one import can reuse an id. The check above sees only what the doc had
before the import, so the first is applied, the second parks, and the dep brings it over
the first with the same assertion as a local write, from a single forged update.

An exporter writes each peer's changes in counter order, so
`check_changes_agree_with_each_other` first makes one pass over the counters, one hash
operation per change. Only a blob out of order takes the second pass, which holds spans
the way `PendingChanges` does (`ClaimedSpans`) and compares overlaps from the version
vector on. Below the version vector both sides were already compared with the op log,
and under `JsonLossy` they may legitimately differ from each other.

## What "equal" means

Both sides can split and merge the same history differently, so the comparison
is per atom range, not per change:

- **Deps** are compared at every counter where either side starts a change. Inside
  a change the deps are implicitly `p@c-1`, and changes only merge when that holds,
  so `deps_at` is canonical. Lamport and timestamp are not compared: the import
  recomputes lamports from deps, and a merged change keeps only its first timestamp.
- **Ops** are cut at every op boundary of either side and compared by meaning
  (`op_eq`): list insert values element by element (each op owns its
  `ValueSlice`), text by bytes, and a one-atom delete ignores the sign of
  `signed_len`, because slicing a reversed delete down to one atom yields `-1`
  where a forward one yields `1`.
- **Value payloads** use the caller's `ImportedValues` mode. `Exact` is the
  default for binary `import`, `import_with`, fast updates, fast snapshots, and
  `import_batch`, and it is always available (not test-only). It is structural
  equality: finite doubles use `==` (a 1 ULP gap is rejected), large integers
  stay distinct, binary is not a list, non-finite doubles are not null, a marker
  string is not a Container, and unknown payloads are compared. `JsonLossy` is
  used by `import_json_updates` and by `import_with_history_mode` /
  `import_batch_with_history_mode` when the caller opts in, because a later
  binary export can carry values degraded by an earlier JSON hop. It accepts,
  symmetrically and recursively inside lists, maps, and rich-text styles:
  `Binary` versus a list of the same u8 numbers, non-finite doubles (`NaN`,
  positive or negative infinity) versus `Null`, and `Double(d)` versus `I64(i)`
  when `d == i as f64`. JS numbers erase the distinction between an integral
  Double and I64; JSON text written by JS also drops the `.0`
  (`crates/loro-wasm/src/convert.rs`, `js_json_schema_to_loro_json_schema`;
  `LoroValueVisitor` in `loro-common/src/value.rs`). Two I64s also match when
  they convert to the same f64 and at least one has `|i| > 2^53`: a JS-number
  relay can round `2^60 + 1` to `2^60`. Inside `[-2^53, 2^53]`, distinct
  integers still differ and Double/I64 equality is exact numerically. Finite
  Double/Double stays `==` in both modes. `serde_json`'s `float_roundtrip`
  feature is on the normal dependency, so a true JSON text round trip of a
  finite double keeps its bits and passes `Exact`.
- **Large-integer JSON paths.** Native JSON text preserves an I64, but parsing it
  into JS numbers can lose precision. The WASM object importer uses
  `serde_wasm_bindgen::Deserializer::deserialize_any`, which reads numbers outside
  JS's safe integer range (`|n| > 2^53 - 1`) as Double. The generic
  `js_value_to_loro_value` also returns Double for `|n| > 2^53`, but returns I64
  at the boundary; a JS-written JSON string can decode a rounded I64 instead.
  Both representations are tolerated, including JS's shortest decimal spelling
  (for example `JSON.stringify(2 ** 60)` writes `1152921504606847000`, which converts
  to the same f64). The direct WASM schema exporter currently uses
  `serde_wasm_bindgen::Serializer::serialize_i64`, which rejects unsafe I64s
  rather than rounding them. The tolerance covers history relayed through JS
  parsing native JSON; it does not change that exporter's existing limitation.
- **Ambiguous payload kinds.** A valid `🦜:cid:` string becomes a Container in
  JSON (`LoroValueVisitor::visit_str`). JSON peer compression can then reinterpret
  the string's peer as an index, so a marker-string versus Container pair is
  accepted without comparing the encoded id. Two Containers or two strings still
  must match. Unknown-op payloads (`FutureInnerContent::Unknown`) are not
  compared in `JsonLossy`: they may carry nested LoroValues or arena references
  whose meaning this version cannot establish. The op's prop and container still
  must match. `Exact` compares the payload. This retains the prior JSON bypass
  for that opaque kind; a later binary relay of it is accepted only when the
  caller opts into `JsonLossy`.
- **Counter ops** carry tagged `OwnedValue::F64`/`I64` values in JSON
  (`encoding/json_schema.rs::decode_op`), both decoded to `Counter(f64)`.
  Finite increments therefore remain comparable numerically, including integral
  increments. NaN equals NaN. A nonzero counter is not treated as zero: integer
  encoding is used only when `fract() == 0.0` (and the existing magnitude bound
  holds), so `±1e-17` is not stored as `I64(0)`. Applying that delta does not
  treat a magnitude below `f64::EPSILON` as an empty diff. Already-corrupted old history
  cannot be recovered. A non-finite counter increment serialized as JSON text is
  already invalid at decode because its tagged f64 becomes null; this is not a
  new known-history rejection.
- **Other values** are compared exactly. Unlike main's blanket JSON value bypass,
  JSON imports now reject genuinely different representable payloads. Text,
  op container ids, positions, keys, element ids, style metadata and deps still
  must match on every import path.
  The known prefix is then trimmed, so the receiver keeps its own values.
  This fixes the 1.16.4 rejection of JSON-relayed history without changing the JSON
  format: a JSON importer still gets a list instead of `Binary` (including mergeable
  container markers), `Null` instead of a non-finite double, an I64 instead of an
  integral Double, a rounded large integer, or a Container instead of a
  marker-looking string. It does not repair that importer's lost values or make
  its state identical to the binary receiver's.
- History below the shallow root, or any local change `get_change` cannot find,
  is not compared. The import then behaves as it did before this check.

## What it does not catch

- **Conflicts indistinguishable from JSON loss, only under `JsonLossy`.** Two
  actual writers using the same op id can store Binary versus a list of the same
  bytes, NaN/positive or negative infinity versus null, or an integral Double
  versus the matching I64. Different large integers (or a large integer versus a
  Double) also pass when they convert to the same f64 and at least one is
  outside `[-2^53, 2^53]`, such as `2^60 + 1` versus `2^60`; distinct integers
  inside that range remain checked exactly. This ambiguity exists even when both
  writers stored I64s directly. Default binary import rejects it (`UsedOpID`).
  It passes only for `import_json_updates` or an explicit `JsonLossy` binary
  import. A valid marker-string versus Container pair and differing unknown-op
  payloads also pass under those bypasses. The receiver keeps its own prefix;
  the conflicting value (or its type) remains different, while unrelated matching
  values stay the same. `Exact` must not leave the documents with equal version
  vectors and different values.
- **A dep this doc has for another reason.** A local `7@0` that exists before `9@0`
  arrives satisfies its dep on another `7@0`. The content of that id is not in the
  update, so it is the same case as delta sync, below. The same goes for the deps of a
  dep that has not arrived.
- **Delta sync.** If the conflicting prefix is not in the import (the sender
  exported from the receiver's version vector), nothing can be compared: the
  receiver's vv already claims those ids. A tail position past the end of a
  sequence is then rejected by `validate_diff` and rolled back (loro-dev/loro#1160).
  A tail that happens to land on valid positions is applied and scrambles the
  state. Detecting that needs content hashes in the version vector.
- **Same-peer changes that are concurrent but contiguous** (for example `p@2`
  with deps `[]` on top of a local `p@0..=1`). They do not crash, and are still
  accepted.
- A peer id reused by a local commit after importing is outside import.

## Cost

All imported overlapping peers are compared, including a fully known peer whose
history another peer depends on. No detection limit applies. An import with no
overlap, the normal path, only reads the version vector once per change.

Cold blocks containing only Text inserts can be compared directly from their
encoded container IDs, positions, UTF-8 strings and dependency boundaries
(`OpLog::check_known_text_in_cold_block`, `block_encode::visit_text_insert_block`).
This does not parse changes or allocate strings in the local arena. Both change
and op boundaries remain significant to the comparison. Unsupported op kinds
or any reader error fall back to the general check; already parsed blocks keep
the existing path. A mismatch from this shortcut is returned only after the
whole block passes its eligibility checks. The shortcut never records a
`parse_failures` entry: its length/boundary requirements are stricter than
`decode_block`, so only the ordinary parser can declare history unparsable.
A short imported change that ends inside a cold block uses the general path so
many short changes cannot repeatedly scan the same large block. The cold path
runs only when the imported overlap reaches the local block's end. Short
changes with different encoded bytes still parse their cold block, which
explains the remaining roughly 2.2x `overlap_snap` cost at `FAT=1`.

The comparison still costs O(overlap); it is not a content-hashed version vector.
The release probe is `crates/examples/examples/import_scaling_stress.rs`.
Use its `overlap_snap`, `overlap_partial`, `overlap_mem`, `snap_plus` and `snap_mem`
scenarios with separate old/main/fixed binaries and interleaved runs. Heap is a
process-wide allocator statistic; inspect both the one-import delta and the
repeated-import series rather than interpreting one allocator growth step as a
universal ratio.

### Imports that bring nothing new

Reporting a conflict here is what stops `import` from returning `Ok` for ops it
did not take, so such an import now pays the comparison. Measured with
`reimport_*` in the release probe on Linux x86_64 under shared load, against the
same tree with the check skipped when nothing is new. 3 interleaved process
runs per cell, 3 timed rounds each; medians. Heap is `mallinfo2`.

| Scenario | FAT | Changes | no check ms | check ms | no check KiB | check KiB |
|---|---:|---:|---:|---:|---:|---:|
| reimport_mem | 1 | 64,000 | 0.019 | 0.021 | 0 | 0 |
| reimport_mem | 64 | 64,000 | 1.019 | 1.201 | 0 | 0 |
| reimport_snapshot | 1 | 64,000 | 12.888 | 17.182 | 12 | 9 |
| reimport_snapshot | 64 | 64,000 | 19.241 | 25.157 | 1 | 1 |
| reimport_snap | 1 | 8,000 | 0.007 | 0.812 | 1 | 21 |
| reimport_snap | 1 | 64,000 | 0.020 | 6.386 | 0 | 228 |
| reimport_snap | 64 | 64,000 | 1.029 | 13.035 | 0 | 4,247 |
| reimport_map | 1 | 8,000 | 1.403 | 3.632 | 16 | 1,493 |
| reimport_map | 1 | 64,000 | 8.840 | 29.546 | 172 | 12,633 |
| reimport_tail | 1 | 64,000 | 0.389 | 0.575 | 0 | 155 |
| reimport_tail | 64 | 64,000 | 0.415 | 0.450 | 0 | 0 |

- Byte-identical blocks still skip decoding and comparison (`reimport_mem`), which
  is the common repeat of the same export.
- `reimport_tail` delivers the newest 200 changes again, one blob each: duplicate
  delivery over a sync channel. Its cost is 1.05x to 1.48x.
- `reimport_snap` looks worst because `all_updates` merges this history into very
  few changes, so the old import did almost nothing. The real figure is about
  0.1 microseconds per op, or 0.2 with `FAT=64`, comparing with cold blocks.
- `reimport_map` compares through `get_change`, which parses cold blocks and keeps
  them parsed. A full-history repeat on a cold doc costs about 3x and 0.2 KiB per
  change. An overlapping import with one new op has always paid this.
- With no overlap nothing changes. At 32,000 changes the fixed/no-check ratios were
  `text_stream` 1.01x, `batch` 1.00x, and, for imports that already compared,
  `overlap_mem` 1.05x and `overlap_snap` 0.98x.

### Parked history

Each import checks `claims` once per change it brings past the version vector: one
hash lookup, or nothing beyond it when the peer has nothing parked. `claim` copies the
change once, when it first parks; moving between slots does not touch `claims`.
Measured as above, 3 interleaved process runs, against the same tree without claims.
`pending_repeat` delivers `n` shuffled blobs that all wait for a missing dep, twice,
and reports the second delivery, which compares with every parked change.

| Scenario | Changes | no claims ms | claims ms | Ratio |
|---|---:|---:|---:|---:|
| pending_repeat | 1,000 | 103.374 | 101.697 | 0.98x |
| pending_repeat | 4,000 | 1,725.852 | 1,750.650 | 1.01x |
| ooo | 32,000 | 161.179 | 164.218 | 1.02x |
| batch_rev | 32,000 | 296.898 | 296.074 | 1.00x |
| batch | 32,000 | 246.102 | 246.042 | 1.00x |
| text_stream | 32,000 | 156.023 | 161.852 | 1.04x |
| overlap_snap | 32,000 | 5.034 | 4.905 | 0.97x |
| reimport_map | 64,000 | 31.247 | 32.452 | 1.04x |

### Local writes and the pass over one import

Measured against the tree before them, with separate build directories: a shared
`CARGO_TARGET_DIR` reuses artifacts between trees, and the two binaries come out
identical. 3 interleaved process runs, 5 where the table says so.

| Scenario | Changes | before ms | after ms | Ratio |
|---|---:|---:|---:|---:|
| pending_repeat | 4,000 | 1,773.814 | 1,767.981 | 1.00x |
| ooo | 32,000 | 173.911 | 174.424 | 1.00x |
| batch (5 runs) | 32,000 | 244.420 | 247.338 | 1.01x |
| batch_rev (5 runs) | 32,000 | 299.399 | 305.033 | 1.02x |
| blob_order (5 runs) | 64,000 | 204.371 | 208.214 | 1.02x |
| text_stream with `SEEDSYNC=1` (5 runs) | 32,000 | 165.635 | 167.676 | 1.01x |
| overlap_snap | 32,000 | 4.956 | 4.949 | 1.00x |
| reimport_map | 64,000 | 30.421 | 30.224 | 0.99x |
| local_commits | 20,000 | 18.318 | 17.969 | 0.98x |

`local_commits` writes next to 1,000 parked changes of another peer. `blob_order` is
one blob of 4 peers. `batch` was 1.08x until imports of one change stopped allocating
the map. `text_stream` with a local seed (4,000 changes, about 700 us per import)
measured 1.02x and 1.04x in two rounds of 7. With the write check compiled out it was
1.01x in the same round, and nothing else runs per import there, so this does not
come from a code path of this check.

`check_not_in_txn` and `PendingChanges::awaited` measured 0.96x to 1.01x on top of that
in `pending_repeat`, `ooo`, `batch`, `batch_rev`, `local_commits` and `text_stream` with
`SEEDSYNC=1` (5 interleaved process runs each).

`pending_repeat` is quadratic with or without claims, which is why it stops at 4,000:
4x the changes cost 16x. `try_apply_pending` is called with the ids of parked
changes too, and for each one lifts and re-parks every slot below it. That predates
this check. The first version of `claim` ran on every re-park and cost 1.6x here.

## Tests

- `crates/loro/tests/import_reused_peer_id.rs`: conflicts in imports that bring
  nothing new through every path, attached and detached, and repeated imports of
  known history in every form; conflicts with parked changes (both orders, partial
  overlaps, parked on the same peer, updates that could apply, every import path)
  and repeated delivery of parked updates; the #1118 repro through
  updates, snapshot, `import_batch` and JSON; a same-length conflict that used to
  scramble; a conflicting known change followed by a new one; skipped counters;
  and re-imports that must still succeed (piecewise vs merged change stores, every
  op kind including `NaN` and one-atom reversed deletes, shallow docs).
- `crates/loro-wasm/tests/import_reused_peer_id.test.ts`.
- `crates/loro/tests/import_json_relay.rs`: JSON-to-binary relays through full
  updates, snapshots and batch import using `ImportHistoryMode::JsonLossy`;
  binary values, mergeable text/counter, non-finite/integral doubles, rounded
  large integers as I64 and Double (including the `2^53` boundary, both signs
  and i64 extrema), marker strings with peer compression, tagged counter values
  and unknown payloads, nested/sequence/style values, both comparison directions
  and JSON re-imports, later relay edits, retained prefix history, genuine
  text/value conflicts that must still be rejected, and an exact-path rejection
  of `2^60` versus `2^60 + 1`.
- `tests::import_atomicity::failed_import_batch_restores_which_parked_changes_hold_their_ids`:
  a rollback restores claims in both directions.
- `oplog::known_history::json_lossy_value_tests`: the precise value equivalences
  and rejection boundaries, including nested values, byte-list contents, numeric
  representations, 1 ULP finite doubles, marker-string ambiguity and unknown-op
  prop/container checks.
- `oplog::known_history::two_arena_tests`: cross-arena container identity and
  Unicode atom comparisons, and a dependency mismatch inside a merged import's
  cold prefix.
- `crates/loro/tests/untrusted_depth.rs`: `JsonLossy` comparison of 20,000-level
  values on a 1 MiB stack, equal and conflicting.
- `crates/json-float-consumer`: a normal `loro` dependency, without the `loro`
  crate's dev-only `float_roundtrip`, checks finite-double JSON text then an
  exact binary reimport, and the same `2^60` conflict under both modes.
- `crates/loro/tests/contracts/counter.rs`: encoding `1e-17` and importing the
  binary update yields `1e-17`, not `0`.
- `oplog::change_store::test`: identical blocks stay lazy, dirty caches shadow
  old KV bytes, repeated snapshots allocate only new text in the string arena, and
  `merged_cold_text_overlap_is_accepted_without_parsing_the_known_block` proves
  that an equal merged overlap stays cold before a successful import.
- `block_encode::test::cold_text_reader_errors_fall_back_without_recording_parse_failures`:
  strings whose lengths differ from encoded op lengths and ops crossing change
  boundaries remain readable through `get_change`, as with `decode_block`.
- `cold_history_conflict_inside_a_large_known_prefix_is_rejected` in the public
  reused-peer tests: exact mismatch ID across multiple cold blocks, through
  updates, snapshots, and batches, with subsequent usability checks.
- `cold_text_overlap_with_merged_updates_is_accepted` in the same public tests:
  merged updates with different block bytes accept a cold snapshot prefix and
  leave both documents at equal state and version.

## Measurements

Measured on macOS arm64, `rustc 1.96.0 (ac68faa20 2026-05-25)`, release builds,
`CARGO_BUILD_JOBS=4`. Baselines: 1.16.3 `ad5b2a6d4546473d9f4a96412d2ae6808c8403ec`,
main `c00c9fa501f8d32f68d6255eacb7035a67fb6ab6`, fixed = this change.
The verifier directory was read only; its probe and NOSEED/SEEDSYNC patch were
copied here. Each cell has 3 interleaved old/main/fixed process runs, each reporting
3 timed rounds after a warmup. Tables show medians of process medians.
The machine is shared; recorded one-minute load ranged 3.48-4.37.
Use ratios, not an individual wall time.

The first five scenarios use `FAT=64 REPEATS=4`. Other scenarios use `FAT=1`;
`text_stream` uses `NOSEED=1` (no concurrent seed). `snap_mem` has no warmup:
its time column is the median of each process's four growing-snapshot imports,
then the median over 3 processes. Its first-import latency is separately covered
by `snap_plus`.

| Scenario | Changes | 1.16.3 ms | main ms | fixed ms | fixed/main | fixed/1.16.3 |
|---|---:|---:|---:|---:|---:|---:|
| overlap_snap | 8,000 | 1.757 | 3.095 | 2.328 | 0.75x | 1.32x |
| overlap_snap | 32,000 | 7.377 | 12.803 | 9.504 | 0.74x | 1.29x |
| overlap_snap | 64,000 | 15.462 | 26.566 | 19.440 | 0.73x | 1.26x |
| overlap_partial | 8,000 | 1.568 | 2.249 | 1.852 | 0.82x | 1.18x |
| overlap_partial | 32,000 | 6.546 | 9.233 | 7.676 | 0.83x | 1.17x |
| overlap_partial | 64,000 | 13.401 | 19.139 | 15.429 | 0.81x | 1.15x |
| overlap_mem | 8,000 | 0.387 | 0.397 | 0.393 | 0.99x | 1.02x |
| overlap_mem | 32,000 | 1.549 | 1.567 | 1.567 | 1.00x | 1.01x |
| overlap_mem | 64,000 | 3.107 | 3.125 | 3.082 | 0.99x | 0.99x |
| snap_plus | 8,000 | 2.474 | 3.925 | 1.597 | 0.41x | 0.65x |
| snap_plus | 32,000 | 10.213 | 16.615 | 6.475 | 0.39x | 0.63x |
| snap_plus | 64,000 | 20.615 | 34.627 | 13.397 | 0.39x | 0.65x |
| snap_mem | 8,000 | 1.148 | 1.919 | 0.203 | 0.11x | 0.18x |
| snap_mem | 32,000 | 4.825 | 7.888 | 0.722 | 0.09x | 0.15x |
| snap_mem | 64,000 | 9.572 | 15.917 | 1.581 | 0.10x | 0.17x |
| detached | 8,000 | 6.865 | 8.897 | 6.954 | 0.78x | 1.01x |
| detached | 32,000 | 28.110 | 35.118 | 28.423 | 0.81x | 1.01x |
| detached | 64,000 | 57.046 | 72.767 | 57.281 | 0.79x | 1.00x |
| mlist_batch | 8,000 | 7.460 | 8.909 | 8.993 | 1.01x | 1.21x |
| mlist_batch | 32,000 | 29.524 | 37.508 | 37.358 | 1.00x | 1.27x |
| mlist_batch | 64,000 | 79.806 | 96.798 | 96.028 | 0.99x | 1.20x |
| text_stream | 8,000 | 18.200 | 20.952 | 20.529 | 0.98x | 1.13x |
| text_stream | 32,000 | 73.324 | 85.664 | 82.378 | 0.96x | 1.12x |
| text_stream | 64,000 | 149.703 | 165.534 | 166.158 | 1.00x | 1.11x |
| batch | 8,000 | 29.818 | 29.709 | 29.698 | 1.00x | 1.00x |
| batch | 32,000 | 124.071 | 127.269 | 125.134 | 0.98x | 1.01x |
| batch | 64,000 | 257.137 | 255.777 | 255.857 | 1.00x | 1.00x |

### Retained allocator memory

Process-wide `malloc_zone_statistics().size_in_use`, in KiB. `snap_plus` is the
one-import delta; `snap_mem` is the delta from the loaded base to import 4.
These statistics have allocator growth steps and include state/history caches;
the exact arena regression test verifies that no incoming known prefix survives.

| Scenario | Changes | 1.16.3 KiB | main KiB | fixed KiB | fixed/main |
|---|---:|---:|---:|---:|---:|
| snap_plus | 8,000 | 2,164 | 4,598 | 1,604 | 0.35x |
| snap_plus | 32,000 | 8,682 | 19,192 | 6,452 | 0.34x |
| snap_plus | 64,000 | 21,282 | 41,202 | 16,827 | 0.41x |
| snap_mem | 8,000 | 5,382 | 10,839 | 1,605 | 0.15x |
| snap_mem | 32,000 | 21,547 | 43,384 | 6,455 | 0.15x |
| snap_mem | 64,000 | 47,007 | 91,137 | 16,828 | 0.18x |

Fixed `snap_mem` retained-heap deltas from its loaded base after imports 1/2/3/4
(median of 3 processes, KiB):

| Changes | Import 1 | Import 2 | Import 3 | Import 4 |
|---|---:|---:|---:|---:|
| 8,000 | 1,605 | 1,605 | 1,605 | 1,605 |
| 32,000 | 6,455 | 6,455 | 6,455 | 6,455 |
| 64,000 | 16,826 | 16,827 | 16,828 | 16,828 |

### FAT=1 controls for the original regression shape

Same release/interleaving/round/repeat settings; these reproduce the originally
reported 9 ms and 15 ms regressions without large inserted strings.

| Scenario | Changes | 1.16.3 ms | main ms | fixed ms | fixed/main | fixed/1.16.3 |
|---|---:|---:|---:|---:|---:|---:|
| overlap_snap | 8,000 | 0.266 | 1.071 | 0.594 | 0.55x | 2.23x |
| overlap_snap | 32,000 | 1.056 | 4.438 | 2.360 | 0.53x | 2.23x |
| overlap_snap | 64,000 | 2.170 | 9.080 | 4.828 | 0.53x | 2.22x |
| overlap_partial | 8,000 | 0.247 | 0.673 | 0.445 | 0.66x | 1.80x |
| overlap_partial | 32,000 | 1.043 | 2.725 | 1.667 | 0.61x | 1.60x |
| overlap_partial | 64,000 | 2.181 | 5.479 | 3.372 | 0.62x | 1.55x |
| snap_plus | 8,000 | 0.801 | 1.745 | 0.361 | 0.21x | 0.45x |
| snap_plus | 32,000 | 3.274 | 7.571 | 1.137 | 0.15x | 0.35x |
| snap_plus | 64,000 | 6.492 | 15.340 | 2.236 | 0.15x | 0.34x |

R1 retains some comparison cost above 1.16.3, which did not check known history.
Movable-list element validation is unchanged: `mlist_batch` retains its roughly
1.2x cost relative to 1.16.3. Text streaming and batch controls are comparable to
main. Snapshot decoding is faster than both baselines and repeated imports plateau.

Reproduce with separate binaries named `old`, `main`, `fixed`:

```sh
export CARGO_BUILD_JOBS=4
cargo build -p examples --release --example import_scaling_stress
# Copy each revision's binary to $BENCH_BIN_DIR/{old,main,fixed}.
python3 crates/examples/import_overlap_measure.py \
  --bin-dir "$BENCH_BIN_DIR" --output-dir /tmp/import-overlap-fat64
python3 crates/examples/import_overlap_measure.py \
  --bin-dir "$BENCH_BIN_DIR" --output-dir /tmp/import-overlap-fat1 \
  --fat 1 --scenarios overlap_snap overlap_partial snap_plus
```

Samples: [FAT=64 matrix](benchmarks/import-overlap-cost-fat64.csv),
[FAT=1 controls](benchmarks/import-overlap-cost-fat1.csv).
The runner also saves full probe output and load readings to `raw.jsonl`.
