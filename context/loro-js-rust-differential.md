# loro-js Differential Tests Against Rust

Verified against code 2026-09-28.

`loro-js/tests/richtext-differential.test.ts` runs random multi-peer Text
scenarios in loro.js and in Rust (`loro-crdt`, nodejs WASM build) side by side
and compares them after every step. It is the check for "does loro.js mean the
same thing as Rust" questions about Text positions, deletes, concurrency,
checkout, snapshots, cursors, and events.

## Running it

1. Build the reference: `pnpm -C crates/loro-wasm build-dev` (or
   `build-release`). The tests load `crates/loro-wasm/nodejs/index.js`, or the
   file named by `LORO_WASM_NODEJS`. Only the nodejs target is needed.
2. `pnpm --dir loro-js test` runs the suite; it is skipped when no build exists.
   Set `LORO_REQUIRE_WASM_REFERENCE=1` to fail instead of skipping.
3. Scale with `LORO_DIFF_SEEDS` (default 40), `LORO_DIFF_STEPS` (default 80),
   and `LORO_DIFF_FIRST_SEED`. 40 × 80 takes a few seconds.

CI runs the loro.js suite with the reference required (`pnpm test-loro-js`, part
of `test-all` after `release-wasm`), so a Rust change that alters Text behavior
also shows up here.

## What a scenario does

`tests/support/richtext-differential.ts` generates plain-data actions from a
seed (`generateActions`), so a failure replays exactly. Each peer exists once
per runtime with the same peer ID. Actions:

- `insert`/`delete` at UTF-16 or UTF-8 boundaries (BMP, CJK, astral, and
  multi-scalar emoji), `mark`/`unmark` with before/after/both/none styles;
- `sync` by binary or JSON updates, within a runtime or crossed (Rust imports
  the loro.js peer's updates and loro.js imports Rust's), so each side reads
  the other's positions;
- `checkout` to a recorded version and back;
- full and shallow snapshot export from either runtime, imported into both;
- cursor creation, then cursor resolution after every later step;
- `revertTo` and undo/redo.

After each step it compares the delta (normalized: merged runs, `null`
attributes dropped), UTF-16/Unicode/UTF-8 lengths, cursor positions, and the
atoms of the JSON history (every inserted scalar and mark with its entity
position; Rust groups atoms into ops depending on its encoding state). loro.js
events must rebuild the loro.js state; Rust events are not checked (see below).

`revertTo` must produce the same state on forks of both runtimes, but its ops
may differ, so the replica runs it in one runtime and the other imports the
change. Undo/redo run in loro.js and Rust imports the result: loro.js's
`UndoManager` does not transform against remote edits like Rust's, so exact undo
parity is only an option (`undoParity`).

## Shrinking a failure

`shrinkActions(rust, actions, options)` removes actions while the scenario
still fails with the same first-line message (numbers ignored). A throwaway
Vitest file that calls `generateActions`, `runActions`, and `shrinkActions`
and prints the result is the quickest loop; paste the shrunk action list into a
regression test.

## Rust behaviors the harness works around

The Rust reference has bugs of its own in UTF-16 (WASM) builds:

- A subscribed transaction that merges deletes of astral text can panic with
  "Op/hint length mismatch" (`change_to_diff` in
  `crates/loro-internal/src/txn.rs`), even after unsubscribing. The harness
  subscribes only to loro.js documents.
- `get_text_entity_ranges` advanced the recorded delete `start_id` by the UTF-16
  length of astral text, so Rust can write a delete whose `start_id` names the
  wrong elements. Rust applies deletes by position, and so does loro.js
  (`LoroText._deleteTargets`). Rust still resolves a deleted cursor target
  through the recorded IDs, so the harness skips cursors Rust cannot resolve.
- Rust's shallow import falls back to the recorded delete IDs when they disagree
  with the positions, which can diverge from its own full-history import. The
  harness compares loro.js with the source document instead.

Snapshot-import checkout (`snapshotCheckout`) is off by default until loro.js
can check out a document imported from a snapshot (loro-dev/loro#1126).
