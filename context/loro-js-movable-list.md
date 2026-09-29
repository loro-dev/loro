# loro.js MovableList: Rust-Compatible Model

Verified against code 2026-09-30.

`loro-js/` must converge with the Rust implementation and read and write its
encoding. This article states the Rust MovableList semantics that loro.js must
reproduce, the loro.js model that implements them, and how the differential
suite checks the two against each other. Complexity rules still come from
[loro-js-performance.md](loro-js-performance.md).

Status: the model below, atomic imports, and import validation are
implemented.

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

Invalid `Move`/`Set` handling (Rust loro-dev/loro#1125) is in
[movable-list-op-validation.md](movable-list-op-validation.md); see the
"Validation" section below for the loro.js side.

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
  version transitions for free. A position's `utf16` metric is 1 when it is
  pointed and 0 otherwise, so the user-visible count is the sequence's visible
  metric and user ↔ op index conversion is O(log n) through the existing
  metric queries. `SequenceIndex.refreshMetrics` recomputes one element's node
  and its ancestors when pointedness changes. The `utf8` metric holds the
  tracker delta (below).
- `elements: Map<peer, Map<lamport, MovableElement>>` keyed by element `IdLp`.
  Each element keeps its current winner position, value and value writer, plus
  history arrays sorted by `IdLp` (positions it may point to, and value
  candidates). Winner selection at a version scans candidates from the
  greatest `IdLp` down until one is included, which is the same work as Rust's
  `BTreeSet` range scan.
- Remote ops are applied in causal order. An op's indices count the positions
  alive at its causal version. When that is not the current state (the op is
  concurrent with ops already applied), the state resolves them through a
  tracker, like Rust's `Tracker::checkout`: it keeps a tracker version and
  gives each position a delta, (alive at the tracker version) - (alive now),
  as its `utf8` metric, so `SequenceIndex.atTracked`/`trackedIndexOf` count
  that view in O(log n). Moving the tracker revisits only the positions that
  the ops between the two versions created or deleted, and each applied op
  moves it past itself, so a run of concurrent ops costs
  O((ops + positions they touch) · log n) instead of one O(n) causal view per
  op. An op that sees the whole state, and every version transition, clears
  the tracker through the set of positions with a nonzero delta.
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
  and shown again as a delete plus an insert of the same value, and when
  several imported moves bring an element back to its index, Rust's net
  change cancels and it reports nothing while loro.js reports a delete plus
  an insert. The effect is the same. Getting Rust's exact shape would need an
  O(n) snapshot per import, and Rust composes element updates in `FxHashMap`
  order.
- Revival, as in Rust's `DocState::apply_diff`: a child container that an
  import, checkout or `diff()` makes reachable again (for example a Text brought
  back by a concurrent move) reports its whole state. This is main's generic
  `#completeDiffEntries`; for a MovableList child, reachability at a version
  comes from `MovableListState.visibleValueAt` (winning position alive and
  winning value is that child).
- Snapshot export follows section 8 exactly. Import hydrates alive positions
  (visible and dead) and visible elements with their real position, element
  and last-set IDs, each seeded as the element's only candidate. Hydrated
  state has no tombstones, origins or older candidates (`historyComplete` is
  false), so it is right only from the snapshot's version on. A MovableList
  is therefore a snapshot sequence like Text and List (loro.js #1126,
  `#markSnapshotSequence`); its snapshot names Rust IDs, so the replay of its
  history is comparable to the snapshot state (`sameMovableListStates`).
  Before a version transition touches it, `#prepareSnapshotTransition`
  rebuilds that one container from its own history (`#completeSnapshotSequence`);
  a Text or List only when the transition crosses a snapshot op, a MovableList
  always, since its candidates are partial. Before an import,
  `#prepareSnapshotImport` does the same for each container that a record
  touches concurrently with the snapshot version (its causal view can need
  tombstones the snapshot dropped; loro-dev/loro#1163 for List and Text) or,
  in a MovableList, whose move/set names an element the state lacks. The
  import then applies its records as usual, so its events are the ops' events,
  as in Rust. A container whose replay differs from its snapshot state (a
  snapshot written by loro.js 0.2) stays `unreplayable`, as #1126 describes.
- A shallow root store seeds root-time position and value candidates, the
  counterpart of `record_shallow_root_state`, so retreat to the root keeps
  root-time winners (the MovableList part of Rust #1124 / loro.js #1127).
- Shallow imports follow Rust's `is_before_shallow_root` on the part of each
  change that is not known yet (`#assertImportsNotOutdated`): a change sliced
  at the known version depends on the op before the slice. An import that
  applies no change still creates the root containers of its pending changes
  (`#materializePendingRoots`, Rust's `pending_root_containers_to_materialize`).
  Decoding deferred snapshot history keeps the document's pending changes.

### Undo, `revertTo`, `applyDiff`

`applyDiff` and `revertTo` follow Rust's MovableList `apply_delta` after
loro-dev/loro#1138 (`#applyMovableListMoves`, main's port on the new model;
[movable-list-apply-diff.md](movable-list-apply-diff.md)): delete unclaimed
elements right to left, then place each insert after its predecessor, and turn
a reinserted child container into a `move`, following the container remap
chain.

Undo follows Rust's `undo_internal` for the MovableList containers of an undo
item (`#undoMovableLists`). A is the inverse diff from the span's last op back
to its dependencies, B is the change since the span, and A transformed over B
(A's inserts first, like `transform(.., left_priority = true)`) is applied with
`apply_delta`. Undoing a `move`/`set` therefore inserts the old value as a new
element and deletes the current one, and a child container moves back. Other
containers keep loro.js's op-based undo. Rust's B is its remote diff, the
changes the UndoManager did not make; loro.js transforms a list's inverse only
when such an untracked change touched that list (the UndoManager's
`isTracked`), and then over everything since, which is approximate. An insert
of a child that is still visible outside the delta's deleted ranges is dropped,
Rust's rule for `from_move` inserts that transforms leave behind
(`#withoutLiveChildInserts`).

## Validation and import atomicity

Validation follows Rust loro-dev/loro#1125 (`validate_movable_list_elem_refs_since`;
[movable-list-op-validation.md](movable-list-op-validation.md)).
`#validateMovableListRefs` in `document.ts` runs on every imported change after
integration: FastUpdates, non-initializing snapshots, and `importBatch`
(except the snapshot that seeds an empty document, which Rust does not
validate op by op either). A `Move`/`Set` whose element is unknown, is not a
`movable-list-insert` into the same list, is outside the op's causal history,
or (on a shallow doc) is older than the root but missing from the root state is
rejected with an error. `MovableListState.applyMove` rejects a `from`/`to`
outside the op-index range of the op's causal view before mutating anything. A
`Move`/`Set` of a deleted element is valid and is applied with the
concurrent-op semantics above, the same on every import path.

The element lookup maps `(peer, lamport)` to an op ID by binary search over
the peer's indexed changes (lamports grow with counters). While snapshot
history is deferred only the overlay is indexed; a miss there means the element
comes from the snapshot base, and it is accepted without decoding history when
the list's hydrated state holds it and the op has seen the whole base.
Otherwise history is materialized once. The shallow-root element set is built
once per root store and list.

A rejection rolls back the whole import or batch. `import`/`importBatch` stage
the document state (history indexes, containers, pending changes, version,
subscribers' events, undo stacks) and restore it when any blob fails, so a
rejected import is invisible ([import-batch-atomicity.md](import-batch-atomicity.md),
"loro.js", states both contracts). Rust reports an out-of-range index as
"movable list diff retains N items but state only has M"; loro.js says "out of
range". `loro-js/tests/movable-list-invalid-ops.test.ts` ports
`crates/loro/tests/movable_list_invalid_ops.rs`; run against a WASM build of
`main` with #1125 merged, all of it but the loro.js-only undecodable-snapshot
test passes, error text aside.

Rust rules loro.js does not copy (edge cases, documented rather than ported):
- Rust rejects any List/MovableList/Text op position at or past 1,073,741,822
  when it decodes ops (`InnerListOp::check_positions`), on every import path.
  loro.js rejects such a move only when it applies it, so a detached import
  records it until `attach` fails.
- Rust bounds-checks the composed diff of an import, so an out-of-bounds move
  that a later op of the same import cancels is accepted. loro.js checks each
  op when it applies it and rejects the import.

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
- `diff()` of a child container created in the range omits a map whose keys
  were all set and deleted again in that range; Rust lists those keys as
  deleted. It happens under List, Map and MovableList parents alike (also on
  `main`). The suite skips such map entries when it compares `diff()`.
- Event labels: multi-blob `importBatch` is `by: "checkout"` in Rust, and
  checkout events carry origin `"checkout"` only in Rust. Rust's
  `oplogVersion()` also counts the pending transaction.

Differences in form, not in effect:

- `revertTo`/`applyDiff` apply same-depth containers in Rust's `FxHashMap`
  order over arena indices, so the two engines write the same values with a
  different op order when several same-depth containers change. The suite
  checks such a revert by value on a fork and replicates Rust's ops.
- `diff()` list deltas can put an insert and a delete at one index in the
  other order than Rust's `DeltaRope`, which composes element updates one by
  one (an insert at or past the end goes after trailing deletes, others before
  them) in `FxHashMap` order. `revertTo`, undo and `applyDiff` turn an insert
  before a delete into an insert before the deleted positions, so the two
  engines can place a restored element on different sides of dead positions:
  the reverted value is the same, but later concurrent inserts there can
  interleave differently. The suite checks such a revert by value and
  replicates Rust's ops.
- Event deltas of multi-op imports can differ in shape, or be a no-op where
  Rust reports nothing (see "Versions, events, snapshots"); the suite then
  checks that each engine's deltas turn the previous value into the new one.
- Rust's `diff()` can list a sequence whose delta is empty (two peers
  concurrently moving one element), where loro.js omits the container. The
  suite ignores such entries.

Rust issues found:

- On a shallow document, `checkout(root)` reports `getLastEditorAt` from the
  root list item's peer: `record_shallow_root_state` (`history_cache.rs`)
  seeds the value writer with the list item ID. The full history reports the
  real setter, and so does loro.js. Imports whose diff crosses the root show
  the same wrong writer (for example the last mover), so the suite does not
  compare `getLastEditorAt` on shallow documents.
- `updates-in-range` on a shallow document exports nothing for a span that
  starts in trimmed history: `ChangeStore::iter_blocks` looks up the block at
  the span start and finds none. loro.js exports the part it has. The suite
  starts such spans at the shallow root.
- An update whose changes partly depend on history before the shallow root
  applies the other changes and then returns
  `ImportUpdatesThatDependsOnOutdatedVersion`, so the import is not atomic.
  loro.js rejects the whole blob; the suite retires such a peer.
