# Imports That Reuse Local Op Ids

Verified against code 2026-09-30.

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
- **JSON imports** use `ImportedValues::Lossy`: JSON text does not round-trip every
  value (`NaN` becomes `null`, binary may come back as a list), so value payloads
  are not compared there. Containers, positions, text, keys, ids and deps still are.
- History below the shallow root, or any local change `get_change` cannot find,
  is not compared. The import then behaves as it did before this check.

## What it does not catch

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
