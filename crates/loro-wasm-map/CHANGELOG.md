# loro-crdt-map

## 1.18.0

### Minor Changes

- c55cebf: `LoroMovableList` can read the stable id of each element.

  The id is assigned when the element is inserted and stays with that element when it is moved or its value is replaced. Two rows with the same value remain distinguishable, including after concurrent moves. The id is still there after checking out an older version and after loading a state-only or shallow snapshot, when the insert that created the element may no longer be in the history. An index past the end, or an id that is not visible at the current version, returns nothing.

  - Rust: `LoroMovableList::get_elem_id_at`, `get_index_of_elem_id`, and `get_elem_ids`, using the existing `IdLp` (`L{lamport}@{peer}`).
  - JS: `getElemIdAt`, `getIndexOfElemId`, and `getElemIds`. An id is `{ peer, lamport }`.

  This is the element's id, not the slot id a cursor anchors. `get_cursor` and `LoroList::get_id_at` (JS `LoroList.getIdAt`) return that slot id, `{ peer, counter }`. List diff events are unchanged.

- d33796d: Stop caller input and untrusted updates from panicking, trapping, aborting the process, or silently losing data. The Rust crates `loro` and `loro-internal` move to 1.18.0 with the same changes.

  **Writes can now be rejected.** A local write (`insert`, `set`, `commit`, `applyDiff`, `revertTo`, undo) throws instead of forking history that the doc already depends on:

  - `ConcurrentOpsWithSamePeerID`, in detached editing, when the peer id already has ops after the current version.
  - `ConcurrentOpsWithSamePeerID`, in any mode, while updates are waiting for a dependency (`pending` in `ImportStatus`) and they use the doc's peer id from its next counter on, or depend on such an id. Before, the write succeeded and importing the dependency later corrupted the document or aborted the process.
  - Rust only: `DuplicatedTransactionError` while a transaction opened through `LoroDoc::inner().txn()` is uncommitted.

  To recover: import the missing dependency, after which the waiting updates apply and writes work again, or switch to a fresh peer id with `setPeerId`. A remote can park an update under your peer id to block writes with it; a new peer id is the way out. In Rust, commit the open transaction and retry.

  **Imports can be rejected while a transaction runs.** Rust only: `import`, `import_json_updates` and `import_batch` return `ImportWhenInTxn` while a transaction opened through `LoroDoc::inner().txn()` is uncommitted, and when called from an event or pre-commit callback of the same document. Commit the transaction, or import after the callback returns. Starting a transaction from such a callback returns `DuplicatedTransactionError` instead of deadlocking.

  **`import` returns `UsedOpID` for conflicting data that brings no new ops.** Bytes whose ops reuse ids the document already has with different content used to return `Ok` and drop them silently. They now throw `UsedOpID`, as do updates that conflict with updates still waiting for a dependency, and a single update that contradicts itself. Importing what the document already has, or already holds as pending, is still a no-op. Data relayed through JSON can differ in representation only (numbers, binary, non-finite doubles): `importJsonUpdates` accepts that, and in Rust so do `import_with_history_mode` and `import_batch_with_history_mode` with `ImportHistoryMode::JsonLossy`.

  **`importBatch` is not all-or-nothing.** When one update in a batch fails on its own, the batch throws and keeps what the other updates imported, including a dependency that lets pending updates apply. Only a failure of the final checkout rolls the whole batch back. An error from `importBatch` does not mean nothing was applied, and it does not say which update failed.

  **New `importBatchDetailed` reports each update of a batch.** It imports as `importBatch` does and, instead of throwing for an update that fails, returns `{ blobs, status, rollbackError }`. `blobs[i]` is the outcome of the i-th update you passed:

  - `{ type: "imported", status }`: the update is in the document. `status` is the `ImportStatus` of that update: its `pending` ops wait for a dependency, which a later update of the same batch may have brought.
  - `{ type: "failed", error }`: the update failed on its own, for example with `UsedOpID` or a checksum mismatch. The other updates are not affected.
  - `{ type: "rolledBack" }`: the update imported, but the final checkout failed and the whole batch was rolled back.

  `status.success` is everything the batch imported and `status.pending` is everything the document still waits for when the batch ends. `rollbackError` is `null` unless the final checkout failed. Then it holds the error, the document is as it was before the call, no entry is `imported`, and no event is emitted. The checkout checks the batch as a whole, so it cannot say which update it rejected: import the updates one at a time to find it. Updates that failed on their own keep their `failed` entry in that case.

  Two differences from `importBatch`: an update whose metadata does not decode (`decodeImportBlobMeta` throws for it) is `failed` and the others are imported, where `importBatch` throws before it imports anything. When every update fails that way, nothing is imported and the document is left as it was, including an uncommitted local change. And a batch of one is a plain import, so the state checks it at once and its failure is `failed`, never `rolledBack`. In Rust this is `LoroDoc::import_batch_detailed` and `import_batch_detailed_with_history_mode`, returning `ImportBatchDetail` with one `ImportBlobOutcome` per blob. They return `Err(ImportWhenInTxn)` when the call is refused as a whole, as `import_batch` does. Nothing was tried then.

  **Rust: `UpdateTimeoutError` gains `Loro(LoroError)`.** `LoroText::update` and `update_by_line` return the edit's own error, such as a deleted container or a read-only document, instead of panicking. Exhaustive `match`es on `UpdateTimeoutError` need the new arm.

  **New and deprecated APIs.**

  - Rust: new `LoroDoc::try_attach` and `LoroDoc::try_checkout_to_latest` return the error, and the doc stays detached at its version and usable. `attach`, `checkout_to_latest` and `find_id_spans_between` are deprecated in favor of `try_attach`, `try_checkout_to_latest` and `try_find_id_spans_between`. They still panic on that input, now after releasing the document's locks.
  - JS: `attach()` and `checkoutToLatest()` now throw where they trapped the WASM instance. `findIdSpansBetween` already threw. No JS API is deprecated.
  - New `importBatchDetailed` (Rust: `import_batch_detailed`, `import_batch_detailed_with_history_mode`, `ImportBatchDetail`, `ImportBlobOutcome`), described above.
  - Rust: `checkout` returns `TransactionError` while a transaction opened through `inner().txn()` is uncommitted, where it used to panic.
  - `export({ mode: "snapshot" })` of a detached document throws when its latest version cannot be checked out, where it panicked.

  **Other fixes.**

  - `checkout` and `forkAt` accept frontiers with several ids of one peer or repeated ids, and use the greatest id of each peer. `decodeFrontiers` does the same for bytes that repeat a peer.
  - `LoroText.update` and `updateByLine` throw the edit's error, such as a deleted container, instead of trapping.
  - Frontiers, version vectors and id spans that the document does not have (unknown peers, negative or out-of-range counters) no longer panic or poison the document. `checkout`, `forkAt` and `diff` return an error; `frontiersToVV` returns `undefined`, `vvToFrontiers` returns the last id of each peer, and `getChangedContainersIn` covers the part of the range the document has.
  - In detached mode, an update that the state would reject (a list insert past the end) makes `attach` and `checkoutToLatest` throw, and the document stays usable.
  - A `JsonLossy` import of deeply nested values no longer overflows the stack.

- 204a995: Add opaque `ParsedJsonPath` and `LoroDoc::jsonpath_parsed` to reuse standard-parsed JSONPath syntax while reading current document values.

### Patch Changes

- adf618a: Fix importing one big text insert made right after a concurrent change taking quadratic time (3M characters: ~50 s, now ~0.1 s). The text tracker updated the id-to-cursor map once per rope leaf report, and the rope reported the same leaf once per 256-op fragment; each update walked the whole insert. Repeated leaves are now updated once.
- 6ee4b16: Merge adjacent text runs with equal effective attributes in `toDelta()` after unmarking. Incremental text events still include null attributes to remove styles.
- a2a3ff6: Speed up `checkout`, `diff`, `forkAt`, and concurrent imports across a single large text insert or delete by coalescing contiguous same-effect leaf updates in the rich-text tracker instead of splitting and re-merging the rope leaf at every 256-character fragment boundary.
- 49697b3: Reduce memory use: importing an update into a document opened from a snapshot no
  longer loads its whole history, version vectors keep sharing structure when merged,
  small tables reserve smaller block caches, and parsed ops keep no spare capacity.
- 499461f: Speed up concurrent imports by reusing the current frontier, reducing repeated
  ancestor traversal, and filtering unrelated container history before materializing
  replay version vectors.
- b1f960e: Reject out-of-range signed DeltaInt payloads as corrupted data instead of silently narrowing them to i32. Valid i32 values and accepted nonminimal signed varints keep their existing decoding behavior.
- 8932e46: Fix a panic when creating or moving a tree node between siblings whose fractional indexes were generated with jitter (#1211). `newBetween` now also compares the last byte of the shorter index, which jitter makes random, and `newAfter` accepts an all-0xFF suffix. Indexes generated without jitter are unchanged.
- 0cd0c16: Reject malformed change-header counts before allocating memory during import and batch import. Decode change-header and timestamp delta columns with checked bit reads and arithmetic so damaged updates return decoding errors instead of aborting or poisoning the document.
- 204a995: Reduce transient allocations when evaluating single-node JSONPath fields and filters, preserving multi-node query results and ordering.
- 9a48087: Avoid allocating a traversal stack for scalar value comparisons, removing repeated allocations when merging consecutive rich-text insertions with the same styles. Deep value comparisons remain non-recursive.
- 204a995: Resolve JSONPath root names as literal keys instead of parsing them as slash paths, numeric indexes or tree IDs.
- 204a995: Avoid allocating owned map key strings during value-only JSONPath traversal while preserving ordered snapshot callbacks and mergeable child translation.
- ff51428: Make undo, redo and checkouts to earlier versions of rich text cost the changes since the last one instead of the whole document history, by keeping the text tracker the first one rebuilds from history and replaying only newer ops into it.
- 01d066b: Fix selection restoration after remote edits when undoing the last history item.
- c66b75b: Fix undoing a step that creates and edits a mergeable container so its content does not reappear when the same container is ensured again. Preserve concurrent edits and existing hidden content across undo and redo.
- 35c6eb0: Reject imported changes whose dependencies do not causally include their same-peer predecessor, preventing malformed updates from poisoning later sync exports. Preserve valid transitive dependencies and out-of-order delivery, and recheck affected pending changes before they are unlocked.
- be467a5: Make the Rust JSONPath parser dependencies optional so builds without the `jsonpath` feature no longer require `pest` or `pest_derive`. Keep the default Rust features and WASM JSONPath support unchanged.
- 3774a24: Fix a panic and order-dependent lamports when a pending remote change gets its deps. The lamport now comes from the deps only, and the lamport in the update is ignored.
- 62fb36f: Apply pending updates after initializing full or shallow snapshots, keeping document state and history aligned. Preserve lazy snapshot state and empty roots, and restore pending changes without emitting events when snapshot import or batch validation fails.
- 1aa14ce: fix: reject overflowing or inverted text mark ranges during import decoding
- ab326e1: Reject imported changes that depend on an op of their own peer other than the previous one. Such a change made the import panic with "lamport is not continuous" and poisoned the document lock. The import now returns a decode error and leaves the document unchanged.
- 1cd9636: Return `ImportUpdatesThatDependsOnOutdatedVersion` instead of panicking when an
  import mixes retained shallow-root dependencies with trimmed history, or replays
  an obsolete pending change. Keep valid import progress and pending data usable
  without poisoning document locks.
- 7cb0c86: Fix operation-span queries between retained versions on shallow documents without
  walking pruned dependencies. Return a history error for versions before the
  shallow root, including equal endpoints.
- e42321a: Speed up importing a concurrent change into a document that has seen many peer ids. Turning the replay base into frontiers walked the history back once per peer; the walk now stops at nodes whose version vector is already cached, so a document with hundreds of peers imports in a fraction of a millisecond instead of tens to hundreds.
- c5e9ad7: Reject imported updates with a text style end anchor that does not directly follow its style start. Such a malformed update used to panic in the rich-text diff and leave the document unusable; `import` now returns a decode error and leaves the document unchanged.
- 0b6cc26: Avoid recursive parent queries in the tree history cache so deep trees do not
  exhaust the stack. Add regression coverage for historical operations after
  concurrent tree moves, including the skipped-cycle case from #1117 whose
  replay ordering was fixed in #1152.
- 2e1f365: Prevent automatic undo merging when the wall clock moves backwards, including when `mergeInterval` is zero. Explicit undo groups continue to merge normally.
- d4e8fc5: Changing the peer id during an active undo group no longer splits the new peer's edits into separate undo steps. The group now restarts from the new peer's counter, so one undo reverts everything the group recorded after the change.
- 332e76d: Make undoing a commit that created many lists fast: a retreat (undo, or a
  checkout to an older version) now certifies its target as a critical replay
  base, and the list and movable-list diff calculators trust a tracker seeded
  there instead of rebuilding it from the whole history once for every list
  the undone span touched.
- c9e4d18: Fix UndoManager's merge timer when an import or another history boundary forces a new undo item. Subsequent local edits now use the new item's merge window, while edits that actually merge keep the original window.
- 9f8518c: Keep `onPush` metadata (`value` and `cursors`) on undo and redo items pushed once the stack holds `maxUndoSteps` items. Each such push evicts the oldest item, so the stack length stays the same, and the metadata was dropped; `onPop` then received `null` and no cursors. The binding now attaches the metadata when the top item's counter span is the pushed span.
- 30c6086: Fix grouped undo across split history spans restoring deleted text out of order
  or reviving intermediate text after a causally dependent remote update. Compose
  inverse map writes in execution order so repeated writes to one key restore its
  pre-group value. Preserve remote edits and redo behavior.
- 32abb24: Enforce `setMaxUndoSteps` immediately when lowering the limit and when redo pushes an undo step. Oldest undo items are discarded with their metadata; a limit of zero retains no undo steps while preserving redo history.
- ea1d383: Stop deeply nested data from untrusted updates from aborting the process. `LoroValue` equality, container depth tracking, deep-value reads (`getDeepValue`/`toJSON`) and Map/List/MovableList snapshot export no longer recurse once per nesting level, and container depth no longer overflows past 65,535 levels. Mergeable container names whose ancestors' names together exceed 1 MiB are no longer treated as mergeable (they used to cost quadratic time and memory on import), and creating such a child with `ensureMergeable*` returns an error.
- b1f960e: Check encoded list/map counts against available value bytes before reserving their storage, and share a declared-slot budget across nested and successive collections in one value reader. Malformed counts return the existing corruption error earlier; valid encodings and the wire format are unchanged. This bounds these count-driven reservations, not all decoder memory.

## 1.17.1

### Patch Changes

- 785c44a: Restore the Rust `LoroDoc::state_vv()` return type to `VersionVector`, preserving
  source compatibility with versions before 1.17.0. Keep `try_state_vv()` as the
  fallible API for callers that need to handle unreadable history. The legacy
  method panics on resolution failure after releasing the document locks. WASM
  `version()` continues to return readable errors through the fallible API.

## 1.17.0

### Minor Changes

- 1cf1dc5: Add `LoroDoc.exportPendingUpdates()` (`export_pending_updates()` in Rust) to
  persist remote updates waiting for missing causal dependencies. It returns
  `undefined` in JavaScript (`None` in Rust) when there are no pending operations.
  The result uses the existing updates format and can be restored with ordinary
  `import` after a separately saved snapshot. Export is read-only and does not
  commit local transactions. Ordinary snapshot/update exports continue to exclude
  remote pending operations.

## 1.16.4

### Patch Changes

- 1b38000: `applyDiff` and `revertTo` are now all or nothing: when they throw (for example on an out-of-range position, a diff made for another version of the document, or a diff type that doesn't match its container), the document, its history and its subscribers are left as they were. Uncommitted edits made before the call are kept, still uncommitted. Applying a diff that names a tree parent the document doesn't have now throws instead of trapping.
- 29553d5: Fix panics and lost events involving containers of a type this version doesn't know (created by a newer loro-crdt):

  - `applyDiff` and `revertTo` no longer trap the WASM instance when they would have to create such a container, e.g. a diff that inserts `"🦜:cid:0@1:Unknown(9)"` into a List, MovableList, Map or tree node meta, or that recreates a deleted container holding one. They throw a readable error and leave the doc unchanged. Moves of unknown elements that are already in a MovableList keep working. With `{ fullState: true }`, re-activating a mergeable child that still holds such a container keeps it.
  - `UndoManager.undo()`/`redo()` throw the same error, without changing the doc, for a step that would recreate such a container. That step (with every edit merged into it) is dropped and its edits stay; the next call undoes the step before it, rebased over the dropped step's edits like over a remote peer's (previously the step was applied partially, losing data, and reported success).
  - Importing or checking out updates that create an unknown container and edit it in the same range no longer traps.
  - `subscribe` callbacks no longer lose a whole event batch when it touches an unknown container; only the events that can't be represented are left out.
  - `getCursorPos` returns `undefined` for cursors on Map, Tree, Counter or unknown containers instead of trapping.
  - `parent()` of a container whose parent has an unknown type throws a readable error instead of trapping.
  - Undo/redo no longer traps when an `onPush` cursor points to a missing container.

- 229c8eb: Fix containers that stayed deleted after being revived. After `isDeleted()` was called on the metadata of a tree node under a deleted ancestor, moving the subtree back (locally or through an import) left that metadata reported as deleted, and edits to it threw until the document was reloaded. The same happened to a movable-list child revived by importing a concurrent move. Cached deletions of tree metadata and movable-list children are now dropped whenever a tree move or a movable-list change is applied.
- 1b38000: A root container touched by a document's history is now part of the value after importing that history, even when its ops add up to an empty value (for example text inserted and then deleted). Previously the writing peer showed `{"t": ""}` while a replay of the same history showed `{}`.
- 1b38000: `export({ mode: "updates-in-range" })` no longer panics (a `RuntimeError: unreachable` trap in WASM) when the spans hold several non-contiguous or overlapping ranges of one peer; all the ranges are exported.
- 1b38000: After an import failed and was rolled back, containers created later could show state left over from containers the failed import had touched (for example a new root map showing a tree node's metadata), read as deleted, or trip an internal assertion when the same bad update was imported again. A failed import now leaves the document's containers unchanged.
- 1b38000: Importing updates or a snapshot that reuse op ids the document already has for different content (two clients that shared a peer id) now throws a "ID ... has been used" error and leaves the document unchanged and usable. Previously the import trapped the WASM instance (`RuntimeError: unreachable`) or silently scrambled text. Imports with a change that skips counters of its peer also throw instead of trapping.
- e75df25: Speed up the first import that is concurrent with a local edit to the same map on a shallow doc. The checkout index no longer decodes every map in the shallow root up front; each map's shallow-root entries are added the first time a diff needs that map. On a doc with 100k maps in the shallow root this import went from ~240 ms to ~3.5 ms natively, and from ~2.8 s to ~6 ms in the WASM build.
- 1eb14b3: Fix `revertTo` and undo adding a mergeable child's content twice after its map key was deleted. For example, `revertTo` a version before `map.delete("s")` turned a mergeable text `"hello"` into `"hellohello"`, a counter `7` into `14`, and a list `["keep"]` into `["keep", "keep"]`. They now write only the real difference and keep the child's characters, list elements and tree node ids.

  `applyDiff` accepts a new option, `applyDiff(diff, { fullState: true })`. Use it when applying the result of `doc.diff()` to a document that may still keep a deleted mergeable child the diff restores, such as the same document or a fork: the child's full content is then aligned with that hidden state instead of appended to it. Without the option, `applyDiff` behaves as before: that is right for documents without such hidden state, and for events forwarded to a document that shares the source's hidden state. Event shapes are unchanged. To build a mirror from scratch, use `diff()` or a snapshot.

  `UndoManager` also keeps a peer's own edit undoable after a remote undo restores the child it edited.

- cdbc239: `applyDiff` on a `LoroMovableList` now keeps every child container that the diff moves. Before, a diff that moved two or more children at once, such as `[{ delete: 2 }, { insert: ["🦜:" + b.id, "🦜:" + a.id] }]` to swap two `LoroText` children, could drop one of them or put elements in the wrong order. `UndoManager.undo()` and `redo()` go through the same code, and after some movable-list edits they could panic with an `OutOfBound` error.
- 5c7552d: Importing a malformed movable-list `move` or `set` op no longer aborts the process. These ops used to panic inside the document locks, poisoning them, and the process then aborted during unwind. Now:

  - A `move`/`set` whose element id is not an element of the same list in the op's causal history, or a `move` whose index is past the end of the list, makes `import`, `importJsonUpdates` and `importBatch` return an error. Nothing from that import is applied and the document stays usable. This also holds when the op depends on another change in the same import, which used to skip validation.
  - Insert, delete and move positions of 1,073,741,822 (about 2^30) or more in a List, MovableList or Text op are rejected when the op is decoded, instead of panicking during import. This is a hard limit on sequence length: it also applies when a document reads its own stored history or a snapshot. No peer could import such a document before either.
  - A `move`/`set` that targets an element deleted earlier in its own history is applied the same way a concurrent move/set of a deleted element is: the move brings the element back, and the set is not visible. Every import path, and a replay of the full history, gives the same result.

  Also fixed:

  - `getChangeAtLamport` returned `undefined` or the wrong change for changes that were loaded from a snapshot and not yet read. The change store misread the lamport range of stored blocks, a leftover from the block format change in 1.0. On a snapshot-loaded peer with many blocks, 42 of 669 sampled lamports returned `undefined` and 625 returned the wrong change; all 669 are now correct.
  - A rolled-back import could leave a cached change block behind that made the next import of the same changes panic with "counter should be continuous".
  - Imports of List and Tree ops no longer copy the peer's last change block on every import to keep a rollback copy. Many small List or Tree imports are about 30% faster.

  Performance: movable-list imports now check every `move`/`set` against the document's history. In release benchmarks that makes them about 2–8% slower for many small imports and 0–4% slower for a 120k-op import where about 70% of the ops are moves and sets.

- 27982a5: Fix three rich-text bugs that a randomized comparison with loro.js found:

  - Text inserted next to a style anchor could land on the wrong side of it, depending on an internal cursor cache (native Rust and WASM): for example, after an `unmark` that changed nothing, typing right after a mark that does not expand after put the new text inside the mark.
  - In the WASM build, a delete spanning text from separate inserts could record a `start_id` naming the wrong characters when an astral character (for example, an emoji) ended one of the inserts. Imports apply deletes by position, but shallow snapshot imports and cursors on deleted text use the recorded IDs, so they could disagree with a full import. This fixes new ops only; deletes already in a history keep their IDs (loro-dev/loro#1149).
  - In the WASM build, committing a transaction that merged backspaces over astral characters panicked with "Op/hint length mismatch" when the document had a subscriber or an `UndoManager`.

- 824bb69: Fix a map value disappearing after importing a shallow snapshot and checking out its root. It happened when a key was rewritten after the root with the value it had at the root (for example by `revertTo`) and the op that wrote the root value was trimmed. Checking out a version also now updates the last editor of map entries and movable-list items whose value is equal at both versions, so `getLastEditor` is correct after a checkout, and shallow and state-only snapshots record the right writer in their root state. Snapshots exported by older versions can still carry the wrong writer in their root state; export them again from a full-history document to fix them.
- 8488567: Re-exporting a shallow document at its own shallow root (a shallow snapshot at the same root, or a full snapshot of a shallow doc) now drops map/list containers that were deleted before the root. Shallow snapshots from older versions could still carry such containers, and re-exporting them used to keep that deleted content; it is now filtered with the same rules as a fresh shallow export, while tree node metadata that can still be revived is kept.

  If a shallow document's stored root state is internally inconsistent (a container's recorded parent does not match the container that references it) and some container looks unreachable, nothing is dropped: the root state is exported as it is, like before, so a container that is still referenced is never lost.

- 8ff700f: Shallow and state-only snapshots no longer keep map/list child containers that were deleted before the shallow root. When the export carried a latest-state overlay (more than 256 retained ops, or any state-only export), the check for "containers created after the root" compared creation ids to the root frontiers for exact equality, so almost every stored container was retained in the root state; those snapshots are now smaller.

  Tree node metadata is the exception: a tree node deleted before the root can be revived by a later op (for example, moving a child out of a deleted parent), so the meta maps of all tree nodes, including deleted ones, stay in the root state. Shallow snapshots with at most 256 retained ops can therefore be larger than before when a tree has many deleted nodes. This also fixes shallow snapshots where such a revived node lost its metadata, a checkout into the retained history lost the revived node's children, and importing such a snapshot panicked with "Parent is not registered".

- 1b38000: Fix a deadlock when a subscription is dropped and its callback owned another subscription or an `UndoManager` on the same document: subscriber callbacks are now dropped after the subscriber set's lock is released.
- 892ed30: Reject imported Text ops whose positions lie past the end of the text. An insert, delete or mark past the end used to panic inside the document (a `RuntimeError: unreachable` trap in WASM, leaving the doc unusable). `import`, `importBatch` and `importJsonUpdates` now throw a decode error and leave the document unchanged and usable, like out-of-bounds List and MovableList ops (#1160).
- 6e294c8: Fix tree checkouts that depended on the path taken. Checking out from a version to a concurrent one could keep a tree node the target version does not have, or, with a subscriber, panic with `get_index_by_tree_id(..).unwrap()` in `tree_state.rs` when the diff created a node's child without the node. The tree diff now retreats and replays from the smallest lamport that differs between the two versions instead of from a replay base, which could be a non-critical meet that skipped earlier concurrent ops. Tree checkouts also no longer search the DAG for a replay base.
- 1b38000: Fix missing tree events on import. When an imported move brought a node back out of a deleted subtree, only the node got a `create` event, not the nodes below it, so a tree rebuilt from events lost them. Importing a move of a node under a deleted node also emitted no `delete` event in some cases. This happened when the import had no concurrency with the local document, for example on a peer that only receives updates.
- 1b38000: Fix a panic (a `RuntimeError: unreachable` trap in WASM) when reading a document forked or snapshotted at a version that holds a mark's start but not its end ("unclosed style mark"). Such a start anchor is now decoded like `checkout` keeps it: it styles nothing until its end arrives. A shallow snapshot of a document whose history ends at such a start no longer fails with `FrontiersNotFound`.
- 1b38000: A map event that sets a key to a container of a type unknown to this version (created by a newer `loro-crdt`) is now delivered with its other keys; only the unknown child's entry is left out. Previously the whole map event was dropped, so edits to known keys in the same event never reached subscribers.
- ce6498b: Fix a crash in documents loaded from a snapshot or created by `fork()`: `isDeleted()`, edits, `getContainerById`, and events touching the metadata of a tree node that was created under an already deleted parent threw `RuntimeError: unreachable` ("Parent is not registered"). The parent of such a container is now found from the change that created it. Also, an import rejected by state validation could leave loaded history pointing at containers and values its rollback had removed, so a later export, checkout, or import crashed (#1161). When a change block is present but cannot be parsed (a corrupted snapshot), such a query no longer traps, and later `import`, `export`, `checkout`, `diff`, `revertTo` and `forkAt` calls throw a "cannot parse change block" error instead of working from a partial history. The current state stays readable.

## 1.16.3

### Patch Changes

- a99b911: Reject inserting a container attached to another `LoroDoc` (directly or nested inside a detached container) with a recoverable error instead of trapping the WASM instance. The target doc is left unchanged.

## 1.16.2

### Patch Changes

- 5a80c15: Fix shallow snapshots that exported successfully but could not be imported into an empty document. The shallow root is now always a critical version of the retained history. Previously two shapes picked a root that other retained changes were concurrent with: a version with an odd number of independent heads, and a past version when a branch that forked below it was merged later.
- c7bd1c3: Preserve deleted-container state required by the history retained in `forkAt`
  and snapshot-at exports. Historical diffs and checkouts can traverse deleted
  list and text descendants without a missing-state panic. Containers created
  after the requested frontier remain excluded.

  Restore the source frontier after snapshot-at validation errors and preserve
  explicit detached mode when exporting a document at its head.

- c3ffde6: Fix a panic followed by a deadlock when subscribing to an event source while
  it is emitting, e.g. calling `subscribeLocalUpdates` or `subscribe` from inside
  a callback, or subscribing on one thread while another thread commits or
  imports. A subscriber added during an emit is now registered for later events;
  it does not receive the event that is already being delivered.
- e2e41a3: Fix `LoroText.getCursor` at UTF-16 boundaries around non-BMP characters.
- be0e1ac: Accept tree snapshot state whose sibling nodes are not in fractional index
  order, such as `snapshot`/`shallow-snapshot` exports from loro.js 0.1.0/0.2.0
  after a tree `move()` (loro-dev/loro#1088). The decoder now sorts siblings by
  their (fractional index, idlp) position instead of panicking with
  `assertion failed: last.0 < pos` on the first read. Tree state with duplicate
  node ids or duplicate sibling positions is rejected as a decode error.

## 1.16.1

### Patch Changes

- 80a0e88: Make imports that are concurrent with the current version only on map/counter
  containers as fast as ordinary linear imports.

  Previously any concurrency between the new ops and the current version
  (for example a single unmerged head from another peer) forced every later
  import to retreat to a critical version and replay the whole branch, and the
  map diff scanned every key of every touched map. Per-import cost grew with
  both the branch length and the map size: 400 small imports on a 2000-note doc
  with one stale head took ~2.4s natively; they now take ~70ms, flat per import
  (a linear import of the same chain takes ~50ms).

  The oplog now proves, per container, whether the concurrent old history
  touches anything that needs positional context. When it only touches registers
  the import replays from the current version; the affected maps are still
  resolved from history so persisted state that drops tombstones (deleted roots,
  dead containers) cannot influence the result. Map diffs in the conservative
  path are also restricted to the keys the update touched.

## 1.16.0

### Patch Changes

- f03d283: Speed up `export({ mode: "shallow-snapshot" })` by up to ~20x on container-heavy documents with a large retained history.

  Building the state at the shallow root used to check the live document out
  backwards (latest -> root). That reverse diff makes the richtext/list diff
  calculators rebuild a full CRDT tracker from empty for every container touched
  in the range, which dominated the export cost: on a doc with ~66k containers
  and ~720k streaming-edit ops, shallow export at a mid-history root took ~3.2s
  versus ~1.5ms for a full snapshot. When at least 64k ops are retained since
  the root, the root state is now reconstructed by replaying the pre-root
  history forward into a temporary doc, and the latest state is read from the
  live store directly without moving the document. The pre-root prefix is bounded
  both relative to the tail (at most 16x the retained op count) and absolutely
  (at most 1M ops) and decoded payload size (at most 32 MiB, estimated by
  walking op payloads — recursing into nested values, style values, and commit
  messages — before any value is copied; op counts miss value sizes, since a Map
  write is one atom regardless of payload size), so a document whose
  pre-root history is huge or byte-heavy and unrelated to the tail keeps the
  previous checkout path. The same export drops to
  ~370ms and the produced blob is slightly smaller (~17% on the same fixture).
  With a small retained range — including a root at the latest version — the
  previous checkout path is kept: it is then equally fast and peaks at ~4x less
  memory, so exporting a lazily imported document no longer materializes its
  whole state. Exported blobs remain logically equivalent; detached or
  already-shallow source docs also keep the previous code path.

  Also fixes shallow snapshot export resurrecting, as an empty entry, a root
  container deleted with `deleteRootContainer` before the shallow root.

## 1.15.1

### Patch Changes

- 34b1fae: Cache immutable container ids per JavaScript wrapper to avoid repeated WASM string decoding.
- 48b3c52: Keep malformed `cid:`-prefixed strings returned from `toJsonWithReplacer` as
  plain strings instead of throwing while resolving them as container IDs.

## 1.15.0

### Patch Changes

- 4837d07: Fix unbounded degradation when re-asserting identical text marks over styled ranges.

  `mark` already tried to skip no-op marks, but the check used
  `StyleRangeMap::get_styles_of_range`, which returns `None` whenever the marked
  range spans more than one style-range leaf. On any document whose style ranges
  are already fragmented (i.e. any document with styles), re-asserting an
  identical mark recorded a new op every time.

  Each redundant mark leaves a pair of style anchors in the container state
  forever — they survive snapshots and are never consolidated — and every styled
  read walks all of them. Callers that re-assert marks (for example editor
  bindings syncing mark state) therefore permanently degraded styled reads without
  bound: reading a 724-character paragraph went from ~5µs to ~4ms after 1000
  redundant marks.

  The skip path now falls back to `StyleRangeMap::range_has_key_value`, which
  scans every style range covering the mark and reports whether each position
  already resolves the key to the given value. Attached and detached texts both
  use the new path when the single-leaf fast path cannot answer.

- f4e011f: Fix an O(n^2) slowdown when reading styled text.

  Building the per-range style metadata for `toDelta` / `getRichtextValue` (and
  the sliced variants used to emit text events) deep-copied the whole `Styles`
  map once per style range. Each key in that map owns the set of _every_ style op
  covering the range, while the resulting `StyleMeta` keeps only the LWW winner,
  so the copy scaled with the number of marks accumulated on the container and
  then discarded all but one element of it — O(marks) per range across O(marks)
  ranges. The conversion now borrows instead of cloning.

  Style anchors are never consolidated, so marks accumulate in the container
  state and every styled read paid for all of them. On a 724-character paragraph
  carrying 2000 accumulated marks, a styled read drops from ~103ms to ~1ms; a
  simulated editor binding that re-asserts marks on each keystroke drops from
  ~5ms to ~200us per read after 500 keystrokes.

## 1.14.1

### Patch Changes

- 76cc0f9: Make `importBatch` atomic and never leave the document detached.

  `importBatch` force-detaches the document so each blob only touches the OpLog, then
  reattaches with a single checkout at the end. Two exits skipped that reattach and
  stranded the document in detached mode, where later imports stop being reflected in
  the state, local edits branch off a stale version, and every later `attach()` re-runs
  the same failing checkout:

  - The closing checkout was `expect`ed to succeed. Remote ops that decode fine but are
    rejected by state validation (for example a list insert past the end of the list)
    only reach that validation here, because the blobs were imported while detached, so
    a malformed blob turned into a panic — a `RuntimeError: unreachable` trap in WASM —
    after the document had already been left detached.
  - Any other panic while decoding or applying a blob unwound past the reattach.

  The batch now runs inside an OpLog rollback scope. If the closing checkout cannot
  apply the accumulated changes, the whole batch is rolled back and `importBatch`
  returns the state-apply error with the document still attached and unchanged, instead
  of trapping. A panic inside the batch reattaches before it is re-raised.

  The pending-changes rollback journal now snapshots each touched slot as it was when
  the scope began, instead of undoing individual mutations. The previous two-phase undo
  resurrected changes that the same import both parked and unlocked — routine across a
  batch — leaving pending entries that referenced container registrations the rollback
  had already discarded, and pre-batch pending changes unlocked by a rolled-back batch
  are now re-parked instead of silently dropped.

  `importBatch` of out-of-order updates is also considerably faster: the import preflight
  no longer rescans the whole pending set once per blob, which made such a batch
  quadratic. Draining 12000 out-of-order updates in one `importBatch` goes from ~3.4s to
  ~0.45s, with peak memory unchanged. In-order batches and single `import` calls are
  unaffected.

## 1.14.0

### Minor Changes

- bbf21c3: fix: avoid unreadable WASM traps (`RuntimeError: unreachable executed`) on invalid arguments. `getCursor` with an invalid `side`, `getEditorOf` at an out-of-range position, and accessing an unknown container (created by a newer version of loro-crdt) now throw catchable JS errors with readable messages or return `undefined` instead of trapping the WASM instance.

  feat: on a Rust panic, the full panic info (message, `file:line`, and the complete stack including the Rust call path) is now stored as a JS `Error` on `globalThis.__LORO_WASM_LAST_PANIC__` before the instance traps, so applications can recover readable diagnostics from a caught `RuntimeError`.

  fix: the WASM start hook was never invoked in the `bundler`/`browser`/`base64` builds (the glue checked the JS namespace instead of the wasm exports for `__wbindgen_start`), so no panic hook was installed there at all. The start hook now runs in every build target.

  feat: out-of-memory failures are now reported too. Allocation failure in WASM bypasses the panic hook and traps silently; the package now wraps the global allocator so OOM stores `loro-crdt: out of WASM memory: allocation of N bytes failed` on `globalThis.__LORO_WASM_LAST_PANIC__` and prints it to `console.error` before trapping.

### Patch Changes

- bbcaef6: Fix a panic on import that could permanently break a document's sync.

  `import_changes_to_oplog` defers the changes whose deps are not yet in the DAG
  and hands them to `extend_pending_changes_with_unknown_lamport` to be parked.
  `try_apply_pending` runs in between and applies changes parked by earlier
  imports, which advances the oplog version, so a deferred change can become
  applicable — or already applied — by the time it is parked. Both outcomes used
  to hit `unreachable!`, compiling to `RuntimeError: unreachable` in WASM.

  The re-classification is now handled instead of asserted: a change that became
  applicable is applied and cascaded into whatever it unblocks, an already-applied
  one is skipped, and `ImportStatus` reports only what genuinely stayed pending.

  This mattered most through `import_batch`, which force-detaches the document for
  the duration of the batch and reattaches after the loop. The panic unwound past
  the reattach and left the document detached, so every later import and export
  failed and the document stopped syncing until the process restarted.

## 1.13.9

### Patch Changes

- ddc47ec: Retreat the conservative replay base to the latest single-head critical
  version of the two versions' combined history instead of the beginning of
  history. When an import or checkout involves a genuinely concurrent branch,
  the causal replay now starts at the most recent point that no concurrency
  crosses (in the sense of Eg-walker's critical versions), skipping the
  fully-synced common prefix that the old empty-version fallback replayed.
- ddc47ec: Fix a convergence bug where a movable tree's incrementally maintained state
  could diverge from a full replay of its own oplog. When newly imported
  operations were concurrent with part of the receiving peer's multi-head
  frontier, the diff mode was misclassified as concurrency-free and the tree
  fast path applied the new moves without adjudicating them against the
  existing concurrent branch. The classifier now verifies that every entry
  point of the imported region causally covers the whole current frontier,
  and otherwise retreats the replay base to the latest critical version so
  the competing branches are replayed together.
- ddc47ec: Correct LCA unmatched-branch detection when an explicit dependency already
  covers the same peer's implicit predecessor. Keep those causally newer imports
  on the current replay base, and avoid rebuilding unchanged list-like containers
  when a genuinely concurrent update needs a conservative base containing a large
  common history.

## 1.13.8

### Patch Changes

- 033f43a: Keep the source-map package changelog aligned with the matching `loro-crdt` release.

## 1.13.7

### Patch Changes

- 3986805: Speed up snapshot import. When decoding a Loro snapshot, the redundant per-block SSTable validation (eager block-metadata decode and per-block checksums) is now skipped, because the whole snapshot body is already protected by the document-level checksum verified during decoding. This removes a second hash pass over the data (roughly halving B4 snapshot import time) while preserving integrity guarantees.

  This fast path is internal to Loro's snapshot decoding. The public `MemKvStore::import_all` still verifies every block's checksum; a separate `import_all_unchecked` opts into the unchecked path and is only used where an outer checksum already guarantees integrity.

- 9b6330d: Speed up local text editing (~35% faster on the B4 editing trace). Three hot-path
  changes: the lock-order debug instrumentation is now compiled out of release
  builds (it ran on every per-op lock acquisition); the visible-op count is bumped
  incrementally for local ops instead of recomputing it from the version vectors
  (which also allocated) on every op; and a couple of per-op allocations on the
  text insert/delete path were removed (lazy error-context formatting and inline
  storage for entity ranges).
- c37e76a: Fix a checkout hang after snapshot import. Change-store blocks decoded from a snapshot recorded a wrong end lamport (`lamport_range.1` was set to the start lamport of the block's last change instead of its end). When a change was split across multiple blocks and a lamport-based lookup engaged the binary search path in `ChangeStore::get_change_by_lamport_lte` — e.g. the movable-list diff calculator resolving historical positions during `checkout` — the degenerate range made the search loop forever. The binary search now also caps its steps and falls back to a scan if it ever fails to converge; the fallback considers both unflushed in-memory blocks and the underlying kv store, so lamport lookups (e.g. `getChangeAtLamport`) stay correct for local changes that have not been flushed yet.

## 1.13.6

### Patch Changes

- a4e6174: Fix undo/redo restoring mergeable map children as regular non-mergeable containers.
- 78f6c8a: Fix a potential deadlock when lazy snapshot loading resolves mergeable container parent depths.

## 1.13.5

### Patch Changes

- 1727258: Improve text insert and snapshot import performance by avoiding duplicate text boundary validation and skipping eager imported change block parsing.
- 52d8168: Recover two per-operation editing slowdowns regressed since 1.11.

  Both are constant-factor regressions on the per-op (auto-commit) editing path
  introduced by the lazy-snapshot work in #985, measured against the 1.11.1
  release.

  1. Every `MapHandler`/`ListHandler`/`MovableListHandler` insert validated its
     value with `ensure_no_regular_container_value`, which heap-allocated a `Vec`
     on each call even for scalar values (the common case). A scalar fast-path now
     skips the allocation and traversal entirely. `map create 10^4 key`:
     ~19.4ms -> ~10.7ms.
  2. The per-op text bounds check (`TextHandler::len`/`len_unicode`/`len_utf16`)
     took two `DocState` locks — one to check whether the container state was
     decoded, then another to query the length. These are now consolidated into a
     single `DocState::get_text_len` that takes one lock and one container-store
     lookup. The lazy-snapshot memory behavior is preserved: a still-lazy
     container reads its cached length metadata without materializing the full
     richtext state. `bench_text B4 apply` (per-op text editing): ~389ms -> ~352ms.

## 1.13.4

### Patch Changes

- 4d577ad: Fix two O(n^2) editing slowdowns.

  1. Editing with UTF-16 / UTF-8 (byte) positions (the default in the JS binding)
     validated each position by materializing the entire `[0, pos)` prefix string,
     making every `insert`/`delete`/`splice`/`mark` O(n) and a run of edits O(n^2)
     (regression since 1.12.0). The boundary check now reads the rope's prefix
     caches via the cursor (O(log n)). Unicode-indexed editing was unaffected.
  2. When a subscriber is attached and many edits land on the same container within
     one event batch (e.g. random-position inserts, or many distinct map-key
     writes), building the event cloned the growing accumulated diff on every
     compose — O(n^2) in the number of fragments. The diffs are now composed in
     place. This affected text, map and list events.
  3. Converting a UTF-16 / UTF-8 position within a text chunk to a unicode offset
     scanned the chunk char-by-char, so editing/slicing a large contiguous chunk
     (a big insert, a loaded document, or a long run of typed text that merges into
     one chunk) was O(chunk length) per op. Chunks that contain no astral-plane
     characters (UTF-16) or are pure ASCII (UTF-8) now convert in O(1), covering
     essentially all real-world text (ASCII/Latin/CJK).

## 1.13.0

### Minor Changes

- fa888d8: Add mergeable child containers: child containers created under a map key that converge across peers on concurrent first-write instead of forking. Exposed as `ensureMergeable{Counter,Map,List,MovableList,Text,Tree}` on `LoroMap`. A mergeable child lives at a deterministic `ContainerID` derived from `(parent, key, kind)`, and its visibility is driven by a binary ref the parent map stores at the key, so deletes and kind conflicts resolve through the map's regular LWW. `getOrCreateContainer` is deprecated for lazy map-child creation; migrate those call sites to `ensureMergeable*`. `setContainer` and `insertContainer` regular semantics are unchanged.

## 1.12.4

### Patch Changes

- a6e23b6: Optimize snapshot export for shallow documents by reusing cached shallow-root state instead of checking out to the shallow root and back to latest.

## 1.12.2

### Patch Changes

- cc587ed: Add a browser package remapping so Vite/Rolldown production builds load WASM without top-level await or circular wasm wrapper chunks.

  Also make the base64 entry easier to bundle with plain esbuild, Rollup, and Next.js Webpack by avoiding static Node builtin `require()` calls and top-level await in browser bundles.

- 8f57f4c: Reduce memory usage for read-only access to snapshot-imported documents by avoiding unnecessary lazy container state initialization.

## 1.12.0

### Minor Changes

- 7dfda87: Make update imports atomic across oplog and document state application.

  - `import` and `import_json_updates` now roll back imported oplog changes when state application fails, so malformed updates do not leave the document with oplog/state divergence.
  - Pending changes that are activated during import are included in the rollback boundary when they can affect state application.
  - Import rollback uses conditional guards to avoid adding fixed overhead to successful detached or no-op imports.

### Patch Changes

- 64aa97c: Harden encoding, snapshot, and import paths against malformed input

  - JSON schema import (`import_json_updates`): out-of-range compressed peer indices now return `DecodeError` instead of being silently accepted as raw peer IDs; mismatched `JsonOpContent` vs container type returns `DecodeError` instead of panicking.
  - Outdated binary encoding decoder (`decode_op`): malformed op streams (missing delete iterators, type mismatches) now return `DecodeDataCorruptionError` instead of panicking.
  - Fast snapshot decoder (`decode_snapshot_blob_meta`): truncated or oversized section lengths now return `DecodeDataCorruptionError` instead of panicking on slice indexing.
  - Change store KV import (`import_all`): corrupted `VersionVector`/`Frontiers` metadata now returns `DecodeDataCorruptionError` instead of panicking.
  - Value encoding (`LoroValueKind::from_u8`, `read_str`): invalid byte values and invalid UTF-8 now return `DecodeDataCorruptionError` instead of panicking.
  - `LoroDoc::diff()`: checkout failures during diff calculation are now propagated as `LoroError` instead of panicking; state restore uses `unwrap()` to fail-fast on internal errors.
  - `try_get_text/list/map/tree/movable_list/counter`: now return `None` for wrong root container types instead of panicking.
  - Detached list insert out-of-bounds: returns `LoroError::OutOfBound` instead of panicking.
  - Tree `mov_after`/`mov_before` on deleted node: returns `TreeNodeDeletedOrNotExist` instead of panicking.
  - `JsonChange::op_len`: empty ops array returns `0` instead of panicking.
  - `renew_peer_id`: avoids theoretical collision with `PeerID::MAX`.

- 0977ad1: Fix lock-order panics when JavaScript callbacks re-enter Loro APIs.

  - `opCount()` no longer reacquires the OpLog lock while the current thread already holds a higher-order lock.
  - `LoroText.iter()` snapshots text chunks before invoking the user callback, so callback code can safely read or mutate the document.

- ef100e6: Reduce memory spikes when exporting snapshots from shallow documents.

  When a shallow document is re-exported from its existing shallow root with only a small tail of updates, Loro now reuses the stored shallow-root state instead of decoding all containers just to re-encode the same state.

- 933d5d6: feat: add clearRedo and clearUndo methods
  #921
- 17dc6c0: Fix several edge-case contract violations in document, text, and JSONPath APIs.

  - JSONPath `value(...)` comparisons now handle boolean values consistently with other scalar comparisons.
  - Rich text mark expansion now follows `ExpandType::Before` and `ExpandType::Both` at documented insertion boundaries.
  - Text delta slicing now validates invalid ranges and UTF-8/UTF-16 boundaries before slicing, and public deltas omit removed-style tombstones after unmarking.
  - Detached list and movable-list out-of-bounds operations now return `LoroError::OutOfBound` instead of panicking.

## 1.11.1

### Patch Changes

- 6f5b7a9: Fix production panic regressions

## 1.10.8

### Patch Changes

- 9f68a57: Return errors instead of panicking when diff or checkout targets frontiers before a shallow snapshot root.

## 1.10.6

### Patch Changes

- 85921d7: fix: don't hang when remapping nested containers w same ID (#911)

## 1.10.5

### Patch Changes

- 2d6c235: Fix counter undo after remote updates #906

## 1.10.4

### Patch Changes

- 864b5ca: perf(loro-internal): remove quadratic slow paths in text import/checkout #895
- a34134d: perf: rm the event calling wrapper for ops that will not trigger events

## 1.10.3

### Patch Changes

- 2800a4c: perf: skip useless unmark op #878
- fffaf45: feat: add JSONPath subscription #883

## 1.10.2

### Patch Changes

- 53635dd: fix: toDelta should ignore null style entries #875

## 1.10.1

### Patch Changes

- ca76d86: fix: Empty LoroText attach error #873

## 1.10.0

### Minor Changes

- ce16b52: feat: add sliceDelta method to slice a span of richtext #862

  Use `text.sliceDelta(start, end)` when you need a Quill-style delta for only part of a rich text field (for example, to copy a styled snippet). The method takes UTF-16 indices; use `sliceDeltaUtf8` if you want to slice by UTF-8 byte offsets instead.

  ```ts
  import { LoroDoc } from "loro-crdt";

  const doc = new LoroDoc();
  doc.configTextStyle({
    bold: { expand: "after" },
    comment: { expand: "none" },
  });
  const text = doc.getText("text");

  text.insert(0, "Hello World!");
  text.mark({ start: 0, end: 5 }, "bold", true);
  text.mark({ start: 6, end: 11 }, "comment", "greeting");

  const snippet = text.sliceDelta(1, 8);
  console.log(snippet);
  // [
  //   { insert: "ello", attributes: { bold: true } },
  //   { insert: " " },
  //   { insert: "Wo", attributes: { comment: "greeting" } },
  // ]
  ```

### Patch Changes

- a78d70f: fix: avoid convert panic #858
- ee94ee4: fix: EphemeralStore apply should ignore timeout entries #865
- 9e0a613: fix: Reject symbol-keyed map objects in wasm conversion #855

## 1.9.0

### Minor Changes

- 10a405b: feat: JSONPath rfc9535 #848

  Thanks to @zolero for the thorough implementation of JSONPath support!

  LoroDoc now supports querying and mutating document data using **JSONPath**, following the [RFC 9535](https://www.rfc-editor.org/rfc/rfc9535) specification.

  ### 🧩 API

  ```ts
  // Execute a JSONPath query on the document
  doc.JSONPath(path: string): any[];
  ```

  ### 📚 Query Examples

  Example data setup

  ```ts
  const doc = new LoroDoc();
  const store = doc.getMap("store");

  // Simplified setup for illustration purposes
  store.set("books", [
    {
      title: "1984",
      author: "George Orwell",
      price: 10,
      available: true,
      isbn: "978-0451524935",
    },
    {
      title: "Animal Farm",
      author: "George Orwell",
      price: 8,
      available: true,
    },
    {
      title: "Brave New World",
      author: "Aldous Huxley",
      price: 12,
      available: false,
    },
    {
      title: "Fahrenheit 451",
      author: "Ray Bradbury",
      price: 9,
      available: true,
    },
    {
      title: "The Great Gatsby",
      author: "F. Scott Fitzgerald",
      price: null,
      available: true,
    },
    {
      title: "To Kill a Mockingbird",
      author: "Harper Lee",
      price: 11,
      available: true,
    },
    {
      title: "The Catcher in the Rye",
      author: "J.D. Salinger",
      price: 10,
      available: false,
    },
    {
      title: "Lord of the Flies",
      author: "William Golding",
      price: 9,
      available: true,
    },
    {
      title: "Pride and Prejudice",
      author: "Jane Austen",
      price: 7,
      available: true,
    },
    {
      title: "The Hobbit",
      author: "J.R.R. Tolkien",
      price: 14,
      available: true,
    },
  ]);
  store.set("featured_authors", ["George Orwell", "Jane Austen"]);
  ```

  ```ts
  // 1. Get all book titles
  doc.JSONPath("$.store.books[*].title");
  // → ["1984", "Animal Farm", "Brave New World", "The Hobbit"]

  // 2. Filter: available books only
  doc.JSONPath("$.store.books[?(@.available)].title");
  // → ["1984", "Animal Farm", "The Hobbit"]

  // 3. Filter: books with price > 10
  doc.JSONPath("$.store.books[?(@.price > 10)].title");
  // → ["The Hobbit"]

  // 4. Use recursive descent to get all prices
  doc.JSONPath("$..price");
  // → [10, 8, 12, 9, null, 11, 14]

  // 5. Slice syntax: first three books
  doc.JSONPath("$.store.books[0:3].title");
  // → ["1984", "Animal Farm", "Brave New World"]

  // 6. Membership test: authors in featured list
  doc.JSONPath("$.store.books[?(@.author in $.store.featured_authors)].title");
  // → ["1984", "Animal Farm", "Pride and Prejudice"]

  // 7. String match using `contains`
  doc.JSONPath("$.store.books[?(@.title contains 'The')].author");
  // → ["F. Scott Fitzgerald", "J.R.R. Tolkien"]
  ```

- 10a405b: refactor!: remove deprecated encoding format in v0.x #849

### Patch Changes

- 3af6a85: fix: WASM loading compatibility for esbuild and rsbuild #851

## 1.8.9

### Patch Changes

- 53f5533: Extract sourcemap to another package
