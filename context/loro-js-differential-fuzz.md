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

Fixed in separate PRs: Fugue insert order after the origin's last child
(#1139), doubled checkout events for concurrent deletes (#1140), a
`checkout(currentFrontiers)` that detached (#1143), shallow roots that were
not critical versions (#1144), local op generation for absent-key map deletes,
zero counter increments, and text delete run order (#1145), deletes imported
while detached (#1146), liveness of containers under deleted ancestors
(#1147), local tree positions and no-op moves (#1148), and events for child
containers that a checkout attaches (#1150, on #1131).

Still open, so `CI_RUNS` does not enable them yet:

- Tree convergence: `loro.js` applies tree moves per node, last writer wins,
  with no cycle check. Two peers that move `x` under `y` and `y` under `x`
  converge to a cycle, so both nodes disappear (Rust keeps `y` as a root with
  child `x`). Rust applies a tree's ops in (lamport, peer) order and skips a
  move whose new parent descends from the target (`TreeCacheForDiff::apply`,
  `diff_calc/tree.rs`). A port needs, per tree, the ops in that order with an
  effective flag. A new op applies directly when it is the newest; otherwise
  the suffix after it is retreated and reapplied. Version transitions replay
  the suffix from the lowest changed op. This rewrites the tree branch of
  `#applyVersionTransition`, which #1127 changes for shallow roots, and the
  tree logic #1131 adds for `diff()`, so it waits for those PRs.
- Tree events: items are a before/after snapshot diff that lists final
  indexes in op order. Importing two roots created at index 0 emits
  `create 0@2 @1, create 1@2 @0` (Rust: `@0, @0`); creating a child and
  deleting its parent in one commit emits the delete first; reviving a node
  omits `create` for its children. Rust records an item per effective op at
  the moment it is applied, plus a `create` for each child of a revived node
  (`TreeState::apply_diff_and_convert`). This belongs with the port above.
- Events for containers that are unreachable after the batch: Rust drops them
  (`DocState::get_path`); `loro.js` still sends them. Needs #1147's liveness.
- Snapshot and shallow checkouts: see #1126–#1128.
- MovableList and rich-text styles are covered by their own reworks
  (`loro-js/tests/differential/` in #1132, and #1135). `loro.js` also skips a
  movable-list `set` to the current value, which Rust records.

## Rust-side findings

Found while comparing, reproducible with `loro-crdt` alone:

- With a subscriber, `checkout` then `attach`, then `text.delete` across an
  astral character panics with `Op/hint length mismatch` (`txn.rs`); #1135
  fixes the same panic for backspacing inside a transaction.
- With a subscriber, some checkout sequences panic at `tree_state.rs:1075`
  (`get_index_by_tree_id(..).unwrap()` on a `Create`), e.g. a change that
  creates a node and a child after a list op, and a branch that imported only
  that list op.
- Checking out from a version that contains a tree node's creation to a
  concurrent version that does not can keep the node (a direct checkout of the
  second version is correct).
- After `isDeleted()` is queried on the metadata of a node under a deleted
  ancestor, a local move that revives the subtree leaves that metadata marked
  deleted until the document is reloaded.
