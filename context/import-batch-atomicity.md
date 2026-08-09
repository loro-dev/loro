# `import_batch` Atomicity and the Detached-Mode Invariant

Verified against code 2026-08-09.

`LoroDoc::import_batch` (`crates/loro-internal/src/loro.rs`) does not import blobs the
way `import` does. It stops the auto-commit txn, keeps the txn mutex for the whole
critical section, force-detaches with `set_detached(true)`, feeds every blob into the
`OpLog` only, and reattaches with a **single** `_checkout_to_latest_without_commit` at
the end. That is why a batch of N blobs costs one state apply instead of N.

## The invariant

**A batch that started attached must never return, or unwind, with the document still
detached.** A stranded detached document is silently broken rather than loudly broken:
`DocState` stops tracking the `OpLog`, later imports do not show up, local edits branch
off a stale version, and every later `attach()` re-runs the same failing checkout.

Two exits used to break it, both fixed in `BatchImportGuard`:

- The closing checkout was `.expect(...)`-ed. Only that checkout runs state validation
  for a batch, so a blob that decodes fine but is rejected by `ContainerState::validate_diff`
  (e.g. a list insert past the end of the list) turned into a panic — a
  `RuntimeError: unreachable` trap in WASM — *after* the doc was left detached.
- Any panic while decoding/applying a blob unwound straight past the reattach. See the
  `unreachable!` fixed in `try_apply_pending` (`src/oplog/pending_changes.rs`) for the
  shape this took in production.

## Why a batch-wide rollback scope

`DocState` is untouched while the batch runs, so if the closing checkout fails the state
is still at its pre-batch version and the `OpLog` is the only thing that moved. Undoing
the `OpLog` therefore makes the two agree again, which is what lets the doc stay
attached. `import_batch` opens `OpLog::begin_import_rollback` before the loop and
`BatchImportGuard::finish` either commits it or rolls the whole batch back and returns
the state-apply error.

Consequences to keep in mind:

- Rollback scopes **cannot nest**: `begin_import_rollback_with_arena` overwrites the
  journal and the matching commit/rollback clears it. `update_oplog_and_apply_delta_to_state_if_needed`
  (the legacy `OutdatedRle`/`OutdatedSnapshot` path) therefore checks
  `OpLog::has_import_rollback` and skips its own per-blob scope inside a batch. A legacy
  blob that fails mid-decode inside a batch keeps its partial prefix until the batch
  ends — the same weaker guarantee the modern `Fast*` detached path already had.
- The rollback discards the *whole* batch, including blobs that imported cleanly. That
  only happens on the closing-checkout failure, where the alternative is an unusable doc.
- After `rollback_import` the shared `self.diff_calculator` still caches ranges against
  the rolled-back history, so `finish` replaces it with a fresh `DiffCalculator::new(true)`.
- `PendingChangesRollback` (`src/oplog/pending_changes.rs`) must be a **single
  chronological log replayed in reverse**, not separate added/removed phases. One scope
  can park a change and later unlock it — rare within a single blob (the lamport-ordered
  main pass applies deps first), routine across a batch, where later blobs unlock what
  earlier blobs parked. Two-phase undo resurrects those scope-local changes on rollback,
  leaving parked `Change`s whose `ContainerIdx` registrations the arena rollback already
  truncated (dangling indices — a corruption vector on the next unlock). The batch must
  also *re-park* pre-batch pending changes it unlocked, or they are silently dropped and
  the doc diverges when their deps arrive; the same log handles both directions.

## Why `catch_unwind`, not `Drop`

Cleanup cannot run *while* unwinding. `std::sync::MutexGuard::drop` poisons its mutex
when `std::thread::panicking()`, so a `Drop` impl that reattaches would poison the
OpLog/DocState/txn locks one by one and then panic on the next `LoroMutex::lock`
("poisoned LoroMutex") — a second panic during unwind aborts the process. The blob loop
runs inside `std::panic::catch_unwind`; `finish` runs after unwinding has stopped and
the original payload is re-raised with `resume_unwind`. `BatchImportGuard::drop` is only
a fallback and deliberately does nothing while panicking.

On `wasm32-unknown-unknown` panics are traps, not unwinds, so `catch_unwind` never fires
there: a genuine panic still leaves the WASM instance and the document broken. What the
fix buys in WASM is that the common malformed-blob case is now an `Err` returned through
`importBatch` rather than a trap.

## Tests

- `crates/loro-internal/src/tests/import_atomicity.rs`:
  `import_batch_with_unappliable_update_stays_attached_and_rolls_back` (also pins that
  batch-parked pendings do not survive the rollback),
  `import_batch_panic_leaves_doc_attached` (uses the `panic_at_batch_import_blob_for_test`
  failpoint), `import_batch_keeps_explicitly_detached_doc_detached`,
  `failed_import_batch_reparks_prebatch_pending_changes`,
  `failed_import_reparks_only_preexisting_pending_changes`.
- `crates/loro/tests/contracts/sync_import.rs`:
  `import_batch_failure_leaves_doc_attached_and_unchanged`.
- `crates/loro-internal/src/oplog/pending_changes.rs`: the `import_batch_*` regressions
  that assert the doc is attached after a batch.
