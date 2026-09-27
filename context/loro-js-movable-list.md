# loro.js MovableList: Rust-Compatible Model

Verified against code 2026-09-30.

`loro-js/` must converge with the Rust implementation and read and write its
encoding. This article states the Rust MovableList semantics that loro.js must
reproduce, the loro.js model that implements them, and how the differential
suite checks the two against each other. Complexity rules still come from
[loro-js-performance.md](loro-js-performance.md).

Status: the model below is implemented. Import validation and atomic rollback
("Validation and import atomicity") land in follow-up changes.

## Rust semantics (the contract)

A MovableList is two CRDTs glued together
([`movable_list_state.rs`](../crates/loro-internal/src/state/movable_list_state.rs)):

- **List items (positions)** form a Fugue sequence. Every `Insert` op creates
  one position per value (ID = op ID + offset). Every `Move` op creates one new
  position (ID = the move op ID). Positions are deleted by `Delete` ops and by
  the source side of a `Move`. The diff calculator's tracker treats a move as
  "delete the item at `from`, then insert at `to` in the view after that
  delete" (`Tracker::move_item` in
  [`tracker.rs`](../crates/loro-internal/src/container/richtext/tracker.rs)),
  and it deletes **by index in the op's causal view**, not by ID
  (`CrdtRope::delete`).
- **Elements** are keyed by `IdLp` of their `Insert`. Each element has
  - `pos`: the winning position, the LWW maximum by `IdLp` = `(lamport, peer)`
    over its insert position and every move position that targets it;
  - `value`/`value_id`: the LWW maximum over the insert value and every `Set`.
- A position is **pointed** when its element's winning `pos` is that position.
  User-visible items are exactly the positions that are alive in the tracker
  and pointed. An alive, unpointed position is a *dead* list item: it still
  counts in op indices (`IndexType::ForOp`) but not in user indices
  (`IndexType::ForUser`).
- An element is visible iff its winning position is alive. Deleting that
  position hides the element. A concurrent move with a greater `IdLp` creates a
  new alive position and revives it. This is why concurrent delete + move gives
  `["a","c","B"]` in `concurrent_delete_and_move_resurrects_moved_element`.
- Checkout uses the same rule per version
  (`MovableListHistoryCache::last_pos` / `last_value` in
  [`history_cache.rs`](../crates/loro-internal/src/history_cache.rs)).
  On a shallow document, `record_shallow_root_state` seeds each root element's
  position and value as a candidate for versions at or after the root.
- Local ops (`MovableListHandler` in
  [`handler.rs`](../crates/loro-internal/src/handler.rs)) encode **op
  indices**: `insert` at `convert_index(pos, ForUser, ForOp)` (the list end is
  the op length, after trailing dead items); `move(from, to)` stores the op
  indices of the element and of the current user item at `to`; `delete`
  emits one `Delete(position id, op index, 1)` per element, adjusting the op
  index for the elements already deleted by the same call.
- `getCreatorAt` = element ID peer, `getLastMoverAt` = winning position peer,
  `getLastEditorAt` = `value_id` peer. `getCursor` anchors the **position** ID.
- `apply_delta` for a MovableList
  ([`movable_list_apply_delta.rs`](../crates/loro-internal/src/handler/movable_list_apply_delta.rs))
  is used by `revertTo`, `applyDiff` and undo. It inserts before it deletes.
  A child container that the delta deletes and reinserts becomes a `move`.
  Plain values are reinserted as new elements.
- Snapshot state (normative: [docs/encoding-container-states.md](../docs/encoding-container-states.md)
  section 8) lists every alive position in order, dead ones included, plus
  element IDs and last-set IDs when they differ from the position/element ID.

Invalid `Move`/`Set` handling (Rust #1125, open when this was written) is in
[movable-list-op-validation.md](movable-list-op-validation.md) once merged; see
the "Validation" section below for the loro.js side.

## Why the old loro.js model diverged

loro.js used to store MovableList elements directly in the Fugue sequence and
physically moved them (`SequenceIndex.moveVisible`). It had no separate
positions, so it could not represent a move of a concurrently deleted element,
a delete that targets a position an element has already left, or a dead list
item. Concretely:

1. concurrent delete + move lost the element (`["a","c"]` instead of
   `["a","c","B"]`);
2. importing Rust's "move, then delete" left the element (`["b","a"]` instead
   of `["b"]`), because the delete targeted the old position ID;
3. snapshots wrote every position ID as the element ID and every last-set ID
   as the element ID, and ignored dead items on read;
4. `getLastEditorAt`/`getLastMoverAt` returned the creator;
5. checkout/diff/`revertTo` replayed or LIS-reordered elements instead of
   selecting winners, and shallow retreat had no root-time fallback;
6. undo ignored `move`/`set`, and `applyDiff` treated a MovableList like a List.

## loro.js model

`loro-js/src/runtime/movable-list.ts` owns the state; `LoroMovableList`
(`containers.ts`) delegates to it. `LoroMovableList` still extends `LoroList`
for API compatibility. Its inherited element sequence is unused, and reading
`_sequence` on it throws, so a code path that forgets the MovableList branch
fails loudly.

- `positions: SequenceIndex<MovablePosition>`: the Fugue sequence of positions.
  `deleted` means "not alive in the tracker"; deletion records (`deletedBy`)
  hold the delete or move op that removed it, which gives causal views and
  version transitions for free. A position's `utf16`/`utf8` metrics are 1 when
  it is pointed and 0 otherwise, so the user-visible count is the sequence's
  visible metric and user ↔ op index conversion is O(log n) through the
  existing metric queries. `SequenceIndex.refreshMetrics` recomputes one
  element's node and its ancestors when pointedness changes.
- `elements: Map<peer, Map<lamport, MovableElement>>` keyed by element `IdLp`.
  Each element keeps its current winner position, value and value writer, plus
  history arrays sorted by `IdLp` (positions it may point to, and value
  candidates). Winner selection at a version scans candidates from the
  greatest `IdLp` down until one is included, which is the same work as Rust's
  `BTreeSet` range scan.
- Remote ops are applied in causal order:
  - `Insert`: Fugue insertion at the op index in the op's causal view, then new
    elements.
  - `Delete`: deletes the target position IDs. Rust deletes by index; the two
    agree for honest ops.
  - `Move`: deletes the position at `from` in the causal view (faithful to Rust,
    including a forged `from`), inserts the new position at `to` in the causal
    view extended by the move itself, then runs LWW on the element's position.
  - `Set`: LWW on the element's value.
- Local ops compute op indices exactly as `MovableListHandler` does, so encoded
  ops are interchangeable with Rust's.

### Versions, events, snapshots

- Version transitions (checkout, `diff`, `revertTo`) toggle only the positions
  created or deleted by the retreated/forwarded ops. They then recompute the
  winners of the touched elements at the target version and refresh the
  pointedness of their old and new positions. That costs
  O((affected ops + affected candidates) · log n), with no replay and no LIS.
- Events come from the same primitive state changes. Hiding a visible item
  deletes it at its user index. A new winning position follows Rust's
  `convert_update_to_event_pos`: insert at the new index (plus one when it
  follows the old one), then delete the old item, so a move onto the same
  slot cancels. A value change is a delete plus an insert. Transitions delete
  in descending before-index order, then insert in ascending after-index
  order. Rust derives an import's event from the net change while loro.js
  composes the ops, so a multi-op import can show an element that was hidden
  and shown again as a delete plus an insert of the same value; the effect is
  the same. Getting Rust's exact shape would need an O(n) snapshot per import.
- Revival, as in Rust's `DocState::apply_diff`: in an import or checkout
  batch, a child container of an element that was hidden when the batch
  started and is visible at its end reports its whole state (and so do its
  descendants), for example a Text brought back by a concurrent move. `diff()`
  does the same.
- Snapshot export follows section 8 exactly. Import hydrates alive positions
  (visible and dead) and visible elements with their real position, element
  and last-set IDs. Hydrated state has no tombstones, origins or candidate
  history, so the container is marked incomplete. Version transitions on it
  replay history once. `LoroDoc.#snapshotStateVersion` records the hydrated
  version; `#needsHistoryReplay` replays history before an import (or an
  attach) containing a record that does not depend on that whole version, or a
  MovableList move/set naming an element the state lacks. Such an op's causal
  view can need tombstones the snapshot dropped. The same rule fixes List and
  Text, which had the same gap. `#discardDeferredSnapshotState` (also in
  loro.js #1126) drops the lazy state first so it is not hydrated again on top
  of the replay.
- A shallow root store seeds root-time position and value candidates, the
  counterpart of `record_shallow_root_state`, so retreat to the root keeps
  root-time winners (the MovableList part of Rust #1124 / loro.js #1127).

### Undo, `revertTo`, `applyDiff`

`applyDiff` and `revertTo` port Rust's MovableList `apply_delta`
(`applyMovableListDelta` in `document.ts`): insert first, defer deletes, and
turn a reinserted child container into a `move`, following the container
remap chain.

Undo follows Rust's `undo_internal` for the MovableList containers of an undo
item (`#undoMovableLists`). A is the inverse diff from the span's last op back
to its dependencies, B is the change since the span, and A transformed over B
(A's inserts first, like `transform(.., left_priority = true)`) is applied with
`apply_delta`. Undoing a `move`/`set` therefore inserts the old value as a new
element and deletes the current one, and a child container moves back. Other
containers keep loro.js's op-based undo. B also contains local changes that
Rust's per-item remote diff excludes; see the remaining divergences.

## Validation and import atomicity

Planned; not implemented yet. Import validation follows Rust #1125. A `Move`/`Set` whose element is unknown,
lives in another container, is outside the op's causal history, or (on a
shallow doc) was deleted before the root is rejected with an error. So is a
`from`/`to` outside the op-index range of its causal view. A `Move`/`Set` of a
deleted element is applied with the concurrent-op semantics above.
`import`/`importBatch` stage the document state (history indexes, containers,
pending changes, version, subscribers' events, undo stacks) and restore it
when any blob fails, so a rejected import is invisible
([import-batch-atomicity.md](import-batch-atomicity.md) states the Rust
contract).

## Differential suite

`loro-js/tests/differential/` drives the Rust WASM build (`loro-crdt` Node
target) and loro.js with the same random multi-peer operations and requires
identical values, versions, frontiers, reachable container IDs,
`getCreatorAt`/`getLastEditorAt`/`getLastMoverAt`, and canonicalized events
after every step. Sync crosses engines, so each engine also decodes the
other's updates and snapshots.

It uses the reference that the other loro.js differential suites share
(`loro-js/tests/support/rust-reference.ts`): `crates/loro-wasm/nodejs`, built by
`pnpm -C crates/loro-wasm build-dev`, or the build `LORO_WASM_NODEJS` points
at (for example an unmerged Rust branch). Without a build the suite skips;
`pnpm test-loro-js` sets `LORO_REQUIRE_WASM_REFERENCE=1` and CI runs it after
`release-wasm`, so there it always runs.

```sh
pnpm -C crates/loro-wasm build-dev
cd loro-js
pnpm vitest run tests/differential/movable-list.test.ts   # CI-sized seeds
LORO_JS_DIFF_SEEDS=500 LORO_JS_DIFF_STEPS=200 pnpm vitest run tests/differential/movable-list.test.ts
```

The suite folds away a few differences that are unrelated to the data model.
Each is named in `harness.ts` (`canonicalBatch`) and in the test file's profile
notes, so a fix can remove its normalization.

Files: `engine.ts` (the shared API surface), `harness.ts` (twin peers,
canonical events, comparisons), `fuzz.ts` (action generation, transport,
replay and minimization), `movable-list.test.ts` (profiles). A divergence
saves its trace to the temp directory; replay it with
`LORO_JS_DIFF_REPLAY=<file>` and add `LORO_JS_DIFF_MINIMIZE=1` to shrink it.

## Remaining known divergences

Found by the differential suite on 2026-09-28. The profiles avoid or normalize
them; each normalization is commented in `harness.ts` or `fuzz.ts`.

loro.js, not specific to MovableList:

- Undo: loro.js replays inverse ops for List, Text, Map and Tree, while Rust
  applies stored inverse diffs transformed by later remote changes. Results
  differ when an undo targets items that an earlier undo recreated, or when
  remote changes interleave. A redo also restores deleted List items in
  deletion order instead of document order, `UndoManager.undo()` picks its
  item before committing the pending transaction, and Rust commits redo with
  origin `"undo"`. The profiles run undo on one peer, never chained, after an
  explicit commit.
- `checkout(frontiers)` at the latest frontiers left loro.js detached; Rust
  stays attached. Fixed by loro-dev/loro#1143; the profiles still skip such
  checkouts.
- loro.js emits events and `diff()` entries for child containers whose parent
  element is already deleted; Rust does not. Only reachable targets are compared.
- Cursors: loro.js List/Text add one to a side-1 cursor's offset and anchor an
  end cursor to the last item; Rust does neither, and resolves a MovableList
  cursor whose item was moved away to `undefined`. Not compared.
- Event labels: multi-blob `importBatch` is `by: "checkout"` in Rust, and
  checkout events carry origin `"checkout"` only in Rust. Rust's
  `oplogVersion()` also counts the pending transaction.

Differences in form, not in effect:

- `revertTo`/`applyDiff` apply same-depth containers in Rust's `FxHashMap`
  order over arena indices, so the two engines write the same values with a
  different op order when several same-depth containers change. The suite
  checks such a revert by value on a fork and replicates Rust's ops.
- Event deltas of multi-op imports and of history replays can differ in shape
  (see "Versions, events, snapshots"); the suite then checks that each
  engine's delta turns the previous value into the new one.

Rust issues found:

- On a shallow document, `checkout(root)` reports `getLastEditorAt` from the
  root list item's peer: `record_shallow_root_state` (`history_cache.rs`)
  seeds the value writer with the list item ID. The full history reports the
  real setter, and so does loro.js.
- An update whose changes partly depend on history before the shallow root
  applies the other changes and then returns
  `ImportUpdatesThatDependsOnOutdatedVersion`, so the import is not atomic.
  loro.js rejects the whole blob; the suite retires such a peer.
