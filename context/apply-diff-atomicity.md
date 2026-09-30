# `apply_diff` / `revert_to` Atomicity

Verified against code 2026-09-30 (loro-dev/loro#1154).

`LoroDoc::apply_diff` and `revert_to` (and loro-crdt `applyDiff` / `revertTo`)
are all or nothing. When they return `Err`, the doc's state, history, version
and subscribers are as before the call. Undo/redo (`undo_internal_with`) still
calls `LoroDoc::_apply_diff` directly and keeps what applied.

## Why a rollback, not a pre-check

`_apply_diff` (`crates/loro-internal/src/loro.rs`) turns each entry of a batch
into local ops. Entries fail for many reasons: positions out of range
(`OutOfBound`), a diff made for another version of the doc, a diff type that
doesn't match the container, a missing container, a tree parent the doc
doesn't have. Checking the whole batch first would mean predicting what every
handler's `apply_diff` does while the loop recreates containers and remaps ids
(the unknown-container pre-check in
[unknown-containers.md](unknown-containers.md) already needs that for one
error). Local ops can instead be undone, because nothing outside the doc has
seen them until the transaction commits.

A stale tree diff used to panic on a parent the doc doesn't have
(`TreeHandler::create_at_with_target_for_apply_diff` unwrapped
`is_node_deleted`); it returns that error now, and the batch is rolled back.

## How it works

`LoroDoc::apply_diff_all_or_nothing` runs the batch in the current transaction,
after any uncommitted edits, which must survive a rollback. Committing them first would have been simpler, but it
turns N `apply_diff` calls before one `commit` into N commits (N events, N undo
steps).

1. `begin_apply_diff_scope` marks the transaction
   (`Transaction::begin_rollback_scope`: the next counter and lamport, the
   first-appearance flag) and starts `DocState::begin_local_rollback`.
2. On success the marks are dropped. The ops stay in the transaction as before.
3. On failure, `rollback_apply_diff_scope`:
   - `Transaction::roll_back_scope` takes the transaction's ops out and keeps
     the ones before the batch (slicing an op the batch's first op was merged
     into), restores the event hints the batch touched (their count and last
     hint, recorded on first touch, since the batch's first hint may have been
     merged into it) and the counters;
   - resets the DAG's local version to before the transaction
     (`AppDag::discard_pending_txn`, with the DAG frontiers the transaction
     saved when it began), opens an import rollback scope, and inserts all the
     transaction's ops as a (non-local) change;
   - checks the state out from the batch's last op back to the op before the
     batch (the middle of that change) with a fresh `DiffCalculator::new(false)`
     (the doc's persistent one never sees the change), with the event recorder
     taken out so nothing is recorded;
   - rolls the op log back with `OpLog::rollback_import_keeping_arena` (change
     store, DAG, pending changes, history cache) and applies the kept ops to the
     DAG's version again (`update_version_on_new_local_op`);
   - finishes with `DocState::finish_local_rollback`.

The transaction goes on: its options, `on_commit` and kept ops are untouched,
and the next commit reports only the kept ops.

The checkout costs a multiple of the batch (measured 2x for a text batch, 6x
for a list and movable-list batch); freeing the history cache makes the next
checkout rebuild it. Only the failure path pays either. A successful call pays
two more transaction and state locks (about 60 ns) and a hash lookup per op.

## Reused op ids

The next local ops reuse the counters and lamports of the discarded ops, so
anything keyed by them must not survive:

- Container states: the checkout restores every container the ops touched.
  `InnerStore` records the containers that got new (not loaded) state during
  the scope (`created_journal`), and `finish_local_rollback` removes them, so
  the store is as before, including which roots `get_deep_value` shows. A
  container created by the batch gets fresh state when an op creates it again.
- Counters: the checkout subtracts the increments, which may not give back the
  same float, so their old values are recorded on the first op and set back.
- Parent links: `SharedArena::forget_parents_of_discarded_ops` drops the links
  of normal containers whose id is at or after the batch's first op. Without a
  link such a container reads as deleted without being cached as deleted
  (`dead-container-cache.md`), and the next op that creates it links it again.
  The registrations stay: other code may hold their indices.
- The arena is not rolled back. The checkout may register old containers and
  create state for them; freeing those indices would leave state entries behind
  (loro-dev/loro#1164). The discarded ops' values stay allocated, so the next
  op's values are not adjacent to the previous op's and the two are stored as
  separate ops instead of one merged op.
- `dead_containers_cache` and `alive_containers_cache` are cleared.

## Other threads

The handlers lock the transaction per op, so another thread can edit or commit
while a batch runs. Rolling back would then drop that thread's edits, so the
batch is left partly applied (the behavior before #1154) and a warning is
logged:

- another thread's op in the scoped transaction sets `foreign_ops` (checked in
  `Transaction::apply_local_op` by a thread-local address);
- a commit, import, checkout or export on another thread commits the scoped
  transaction, so the transaction found at failure is not the scoped one;
- a batch that starts while another thread's batch runs in the same
  transaction gets no scope.

Scope ids also key the `DocState` journal, so a stale scope cannot end a newer
one's journal.

## Tests

- `crates/loro/tests/apply_diff_atomicity.rs`: the #1154 repro, type mismatch,
  missing container, detached editing, shallow docs, events, `UndoManager`,
  pending edits (still uncommitted, their commit message, ops and event hints
  the batch merged into, the single event of the next commit), counters,
  containers and tree nodes created by the failed batch and created again,
  twins that make the same edits without the failed call (random stale
  `doc.diff(a, b)` batches, including mergeable children, and a fixed case for
  existing containers), `revert_to`, and another thread editing during failing
  batches.
  `LORO_APPLY_DIFF_ATOMICITY_SEEDS` runs more than the default 120 seeds.
- `crates/loro-wasm/tests/apply_diff_atomicity.test.ts`: `applyDiff` throws and
  leaves the doc, events and undo stack unchanged.
