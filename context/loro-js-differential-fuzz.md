# loro.js Differential Fuzzing

Verified against code 2026-09-28.

`loro-js/tests/fuzz/differential.ts` drives several `loro.js` peers through
random edits, update exchanges, and checkouts, and checks them against
independent oracles. `loro-js/tests/differential-fuzz.test.ts` runs a fixed,
CI-sized seed set by default and larger runs through environment variables.

## Scenario model

A scenario is a list of abstract actions: a local edit (`op`), an update
exchange (`sync`), or a checkout. Each action carries random 32-bit parameters
that are resolved against the acting document when it runs (container, position,
value). Removing actions therefore keeps the rest meaningful, which is what
`minimizeScenario` (delta debugging on the action list) relies on.

- Containers: root Map/List/Text/Tree/Counter/MovableList, and child
  containers in maps, lists, and tree metadata (`FuzzFeatures`).
  MovableList and rich-text marks are off in `CORE_FEATURES`.
- Syncs: `update` from the receiver's oplog version, full updates, one blob per
  missing change in a shuffled order (`import` or `importBatch`), and optionally
  full snapshots into a non-empty document.
- Checkouts go to a random op or to the union of two ops, and either return to
  the latest version or leave the peer detached until its next edit.

## Oracles

- Every peer, after a final full exchange, equals a document that imports all
  updates at once, and one that imports each change separately in a shuffled
  order.
- `checkout(F)` equals a fresh document that imports only the changes in `F`
  (`updates-in-range` from counter 0), for every op of a small history.
- A full snapshot and three shallow snapshots equal the full-history document,
  including checkouts into the retained range and back.
- `EventMirror` applies every event batch to a model keyed by container ID and
  must reproduce `toJSON()` after each commit, import, and checkout. It follows
  the Rust fuzzer's `ContainerTracker` (`crates/fuzz/src/value.rs`): attaching a
  container value restarts that child from an empty state, a movable-list move
  keeps the moved child's state, tree `delete` removes the subtree, and every
  sequence index must be valid when the items are applied in order.

## Rust/WASM reference

With `FUZZ_WASM=1` (or a path to `loro_wasm.js`), each peer also has a
`loro-crdt` twin with the same peer ID. In lockstep mode (default when WASM is
loaded) every concrete edit, sync, and checkout runs on both, and the values
are compared after each action. The first mismatch names the action where the
implementations diverge. After the scenario, a Rust replay, Rust checkouts, and
shallow snapshots exchanged in both directions are compared too, including the
shallow root each side picks. `FUZZ_VALIDATE_HARNESS=1` checks only Rust's own
event mirror; use it after changing `EventMirror`.

Build the reference with
`cd crates/loro-wasm && deno run -A ./scripts/build.ts release nodejs`.
Lockstep requires both implementations to emit the same local ops for the same
API calls; an op-generation difference shows up as `lockstep` or
`lockstep-throws` on the first affected action.

## Running

```sh
cd loro-js
pnpm vitest run tests/differential-fuzz.test.ts            # fixed CI seeds
FUZZ_SEEDS=0-499 FUZZ_FEATURES=core FUZZ_WASM=1 FUZZ_MINIMIZE=1 \
  FUZZ_OUT=/tmp/report.json pnpm vitest run tests/differential-fuzz.test.ts
```

`FUZZ_CHECKS` narrows the oracles (`events,checkout,snapshot,shallow`),
`FUZZ_LOCKSTEP=0` compares with Rust only after the scenario, and `FUZZ_TRACE=1`
prints each action as it runs, which finds hangs. The report groups failures by
signature (oracle plus first differing root or normalized error) and stores the
minimized action list and a trace written as `loro.js` API calls.

Seeds that are fixed in the CI run go into `CI_RUNS[].seeds`; a seed that fails
on `main` for a known reason goes into `pending` with that reason until its fix
lands.

## Divergences found on main (2026-09-28)

Fixed in separate PRs (#1139, #1140, and the `fix/loro-js-*` branches listed in
the pending map): Fugue insert ordering after the origin's last child, doubled
checkout events for concurrent deletes, deletes imported while detached,
`checkout(currentFrontiers)` detaching, the shallow root not being a critical
version, and local op generation (absent-key map delete, zero counter
increment, text delete run order).

Still open, so `CI_RUNS` does not enable them yet:

- Tree: concurrent moves can form a cycle (both nodes disappear, and a later
  local move loops forever); tree event items use final indexes in op order;
  nodes under a deleted ancestor still count as alive; equal fractional indexes
  make `createNode`/`move` throw instead of rearranging; no-op moves emit ops.
- Child containers re-attached by a checkout get a delta relative to their
  hidden state instead of their full state (Rust "revives" them).
- Snapshot and shallow checkouts: see #1126–#1128.
- MovableList and rich-text styles are covered by their own reworks
  (`loro-js/tests/differential/` in #1132, and #1135).
