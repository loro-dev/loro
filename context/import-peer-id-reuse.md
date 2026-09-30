# Imports That Reuse Local Op Ids

Verified against code 2026-10-01.

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

1. **Content.** If the import brings anything new, every imported change that
   overlaps the local version vector is compared with the local changes over the
   overlap. A mismatch returns `LoroError::UsedOpID { id }` with the first
   differing id.
2. **Trim.** The known parts are dropped, as the decoders used to do.
3. **Skipped counters.** A change whose deps are present but whose peer's
   previous counter is not is rejected with `DecodeError`. The main pass of
   `import_changes_to_oplog` would apply it and trip the counter assertion in
   `AppDag::update_version_on_new_change` under the op log lock. The check replays
   that pass on counter ends only. Parked pending changes already wait for their
   own predecessor (`remote_change_apply_state`).

The decoders (`ChangeStore::decode_block_bytes`, `decode_snapshot_for_updates`)
therefore no longer trim. A snapshot with nothing new still returns no changes,
without cloning them.

## What "equal" means

Both sides can split and merge the same history differently, so the comparison
is per atom range, not per change:

- **Deps** are compared at every counter where either side starts a change. Inside
  a change the deps are implicitly `p@c-1`, and changes only merge when that holds,
  so `deps_at` is canonical. Lamport and timestamp are not compared: the import
  recomputes lamports from deps, and a merged change keeps only its first timestamp.
- **Ops** are cut at every op boundary of either side and compared by meaning
  (`op_eq`): list insert values through the arena (offsets differ), text by bytes
  (`unicode_start` is an arena offset), and a one-atom delete ignores the sign of
  `signed_len`, because slicing a reversed delete down to one atom yields `-1`
  where a forward one yields `1`.
- **Value payloads** use `ImportedValues::Lossy` for both JSON and binary imports:
  binary history may have been relayed through JSON before it was exported.
  `ImportedValues::eq` accepts `Binary` versus a list of the same u8 numbers,
  non-finite doubles (`NaN`, positive or negative infinity) versus `Null`, and
  `Double(d)` versus `I64(i)` when `d == i as f64`, in either direction. These
  rules apply recursively inside lists and maps and to rich-text style values.
  JS numbers erase the distinction between an integral Double and I64; JSON text
  written by JS also drops the `.0` (`crates/loro-wasm/src/convert.rs`,
  `js_json_schema_to_loro_json_schema`; `LoroValueVisitor` in `loro-common/src/value.rs`).
  Two I64s also match when they convert to the same f64 and at least one has
  `|i| > 2^53`: a JS-number relay can round `2^60 + 1` to `2^60`. The same f64
  comparison permits an I64 versus its rounded Double. Inside `[-2^53, 2^53]`,
  distinct integers still differ and Double/I64 equality is exact numerically.
  The test-only `ImportedValues::Exact` provides the strict baseline for unit tests.
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
  must match. Unknown-op payloads (`FutureInnerContent::Unknown`) are also not
  compared in Lossy mode: they may carry nested LoroValues or arena references
  whose meaning this version cannot establish. The op's prop and container still
  must match. This retains the prior JSON bypass for that opaque kind and permits
  its later binary relay.
- **Counter ops** carry tagged `OwnedValue::F64`/`I64` values in JSON
  (`encoding/json_schema.rs::decode_op`), both decoded to `Counter(f64)`.
  Finite increments therefore remain comparable numerically, including integral
  increments. NaN equals NaN for binary reimports. A non-finite counter increment
  serialized as JSON text is already invalid at decode because its tagged f64
  becomes null; this is not a new known-history rejection.
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

- **Conflicts indistinguishable from JSON loss.** Two actual writers using the
  same op id can store Binary versus a list of the same bytes, NaN/positive or
  negative infinity versus null, or an integral Double versus the matching I64.
  Different large integers (or a large integer versus a Double) also pass when
  they convert to the same f64 and at least one is outside `[-2^53, 2^53]`, such
  as `2^60 + 1` versus `2^60`; distinct integers inside that range remain checked
  exactly. This ambiguity exists even when both writers stored I64s directly.
  Such a conflict passes the overlap check even without a JSON relay. A valid
  marker-string versus Container pair and differing unknown-op payloads also
  pass under the bypasses above. The receiver keeps its own prefix; the conflicting
  value (or its type) remains different, while unrelated matching values stay the same.
- **Delta sync.** If the conflicting prefix is not in the import (the sender
  exported from the receiver's version vector), nothing can be compared: the
  receiver's vv already claims those ids. A tail position past the end of a
  sequence is then rejected by `validate_diff` and rolled back (loro-dev/loro#1160).
  A tail that happens to land on valid positions is applied and scrambles the
  state. Detecting that needs content hashes in the version vector.
- **Imports with nothing new.** A conflicting duplicate that brings nothing new
  is a no-op and is not reported. This keeps a no-op re-import as cheap as before.
- **Same-peer changes that are concurrent but contiguous** (for example `p@2`
  with deps `[]` on top of a local `p@0..=1`). They do not crash, and are still
  accepted.
- A peer id reused by a local commit after importing is outside import.

## Cost

Imports that don't overlap local history, or overlap only a little, cost the same
as before. An import that re-sends all known history *and* brings new changes
pays for comparing the overlap. On a 10k-change doc that was about +15% (55 → 64 ms)
for both "all updates + 1 change" and "snapshot + 1 change". Probe:
`crates/examples/examples/import_known_history_perf.rs`.

## Tests

- `crates/loro/tests/import_reused_peer_id.rs`: the #1118 repro through
  updates, snapshot, `import_batch` and JSON; a same-length conflict that used to
  scramble; a conflicting known change followed by a new one; skipped counters;
  and re-imports that must still succeed (piecewise vs merged change stores, every
  op kind including `NaN` and one-atom reversed deletes, shallow docs).
- `crates/loro-wasm/tests/import_reused_peer_id.test.ts`.
- `crates/loro/tests/import_json_relay.rs`: JSON-to-binary relays through full
  updates, snapshots and batch import; binary values, mergeable text/counter,
  non-finite/integral doubles, rounded large integers as I64 and Double (including
  the `2^53` boundary, both signs and i64 extrema), marker strings with peer
  compression, tagged counter values and unknown payloads, nested/sequence/style
  values, both comparison directions and JSON re-imports, later relay edits,
  retained prefix history, and
  genuine text/value conflicts that must still be rejected.
- `known_history.rs` unit tests: the precise value equivalences and rejection
  boundaries, including nested values, byte-list contents, numeric representations,
  marker-string ambiguity and unknown-op prop/container checks.
