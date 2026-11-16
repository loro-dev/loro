# LoroDoc Bounded Memory Budget

Verified against code 2026-11-16.

## Overview

`MemoryBudget` (`crates/loro-internal/src/memory.rs`, re-exported by `loro` and `loro-crdt` WASM) lets callers cap and inspect a document's RAM footprint without silently dropping history or corrupting shared transactions.

## `DocMemoryStats` Accounting (14 Fields)

`LoroDoc::memory_stats()` (`crates/loro-internal/src/loro.rs`) returns `DocMemoryStats` with 14 fields:

- **12 RAM component fields** summed into `total_bytes`:
  1. `state_bytes`: materialized full container states in `DocState` + container table overhead (`InnerStore::estimated_state_bytes`).
  2. `value_cache_bytes`: decoded value-only wrappers held in `InnerStore::value_cache_queue` (`InnerStore::value_cache_bytes`, never double-counted with materialized `state_bytes`).
  3. `kv_bytes`: compressed SSTables and memtables across `DocState` and `OpLog` (`ChangeStore`), plus shallow-root KV stores.
  4. `block_cache_bytes`: decompressed SSTable blocks inside the per-document `SharedBlockCache` (`kv-store`).
  5. `change_cache_bytes`: parsed/unflushed change blocks in `ChangeStore` (`mem_parsed_kv` + `changes` / block cache).
  6. `dag_bytes`: `AppDag` nodes and cached version vectors (`oplog/loro_dag.rs`).
  7. `arena_bytes`: `SharedArena` container/value/string arenas (`arena.rs`, `str_arena.rs`).
  8. `pending_bytes`: parked out-of-order updates tracked incrementally in `PendingChanges` (`oplog/pending_changes.rs`).
  9. `history_cache_bytes`: `HistoryCache` frontier/vv index (`history_cache.rs`).
  10. `diff_calc_bytes`: persistent CRDT diff trackers in `DiffCalculator` (`diff_calc.rs`).
  11. `undo_bytes`: stacks tracked by active `UndoManager` instances registered in `LoroDocInner::undo_managers`.
  12. `uncommitted_txn_bytes`: ops and `event_hints` capacity in the currently open `Transaction`. When `memory_stats()` is called on the thread already holding `self.txn`, `self.txn.is_held_by_current_thread()` avoids self-deadlock and the caller supplies `uncommitted_txn_bytes` explicitly (e.g. `try_commit_with`).
- **`total_bytes`**: exact sum of the 12 RAM component fields above.
- **`spilled_disk_bytes`**: out-of-core spilled bytes (0 when using in-memory stores; intentionally excluded from `total_bytes`).

## Stage 1 & Stage 2 Soft Reclamation (Hysteresis & Clock Demotion)

- `effective_high_watermark()` defaults to 85% of `max_doc_bytes`; `effective_target_watermark()` defaults to 70% of `max_doc_bytes`.
- **Hysteresis**: when `total_bytes <= high`, commits skip soft reclamation (`enforce_watermarks_with_budget_locked`). Once `total_bytes > high`, reclamation aims down to `target`:
  - **Stage 1**: evicts decoded value wrappers (`evict_value_cache_to_bytes`), caps/clears `kv_block_cache`, frees `DiffCalculator` cold container trackers (`reclaim_cold_caches`), calls `reclaim_auxiliary_caches()` on `DocState` containers (including `RichtextState::id_to_cursor_cache`), compacts `OpLog` history (`OpLog::compact_history`, skipped while `import_rollback` is active or `AppDag::has_pending_txn()` is true so `ChangeStore::flush_and_compact` never trips on uncommitted ops), frees `HistoryCache`, and trims registered `UndoManager`s.
  - **Stage 2**: if `total_bytes > target`, `InnerStore::demote_cold_states` runs a two-pass clock sweep (clearing `state_referenced` on pass 0, encoding & dropping unreferenced non-empty, non-pinned containers into `self.kv` on pass 1) and flushes `state`/`oplog` KV memtables. Newly created containers recorded in `created_journal_stack` are pinned while inside a rollback scope; empty containers and cleared deleted roots are skipped so they do not bloat KV.

## Runtime quota changes

`set_memory_budget` changes the hard document quota for the next transaction.
An already edited transaction keeps its captured `Transaction::doc_budget_limit`;
setting a quota in a commit callback never retroactively rejects those edits.
An empty auto-commit transaction can adopt the new quota before its first edit.
`try_commit` runs admission, rollback and commit under the same transaction lock;
manual commits and `apply_diff` use the same captured quota. Existing nested
batch scopes prevent a new budget scope from being inserted beneath them.
Single and batch imports perform soft reclamation after closing import rollback
scopes, including when only watermarks are configured. The two-pass cold-state
sweep uses a fixed starting clock hand so advancing the hand never skips slots.
Unbounded undo stacks skip byte estimation during trimming.

## Stage 3 Hard Admission & Atomic Rollback

- **`ImportBlob` (`max_import_blob_bytes`)**: checked pre-decode across `import*`, `import_batch*`, `import_batch_detailed*` (isolated per-blob `Failed`), and `import_json_updates` (via a zero-allocation `CountingWriter`).
- **`PendingPool` (`max_pending_bytes`)**: enforced incrementally during `PendingChanges::import_pending_change` with key/range deduplication; breaching it triggers mid-blob rollback in `OpLog`.
- **`DocBudget` (`max_doc_bytes`)**:
  - **Remote imports & batches**: checked after `DocState` apply and watermark reclaim. Because `oplog.rollback_import(&mut state)` only reverts `OpLog`, callers (`import`, `BatchImportGuard::finish`, `init_snapshot_preserving_pending_with`) silently revert `DocState` first (`revert_state_silently` + `take_events()` + `remove_created(&created)`), then roll back `OpLog`, and reset `DiffCalculator`. When `has_doc_budget` is set, `BatchImportGuard::finish` defers `doc.emit_events()` until after the `DocBudget` check succeeds so subscribers never observe rolled-back batches.
  - **`UndoManager`**: `compose_remote_event` filters out diffs for containers unrelated to `affected_cids` (while preserving `Tree` events) to prevent remote churn on untouched containers from bloating undo stacks, and `set_max_undo_bytes` / `shrink_to_fit` evict oldest items FIFO.
  - **`try_commit` / `try_commit_with` vs infallible `commit`**:
    - `commit()` / `commit_with()` stay infallible (performing Stage 1/2 best-effort reclamation without failing).
    - `try_commit()`, `try_commit_with()`, manual `Transaction::commit()`, and `apply_diff` enforce `max_doc_bytes`. Local transactions under `max_doc_bytes` arm `budget_base_scope` on `Transaction` (`begin_budget_rollback_scope` with `ignore_thread = true`).
    - On rollback (`finish_local_rollback`, `roll_back_scope`, `remove_created`), `LocalRollbackJournal` restores `SharedArena`'s `StrArena` to its checkpoint (`str_checkpoint`), `InnerStore::remove_created` truncates trailing empty slots and excess `Vec` capacity, `Transaction::roll_back_scope` shrinks/clears `event_hints`, and `rollback_transaction_scope_inner` frees transient `HistoryCache` entries so subsequent smaller commits remain unblocked.

## WASM / TypeScript Surface (`loro-crdt`)

- `LoroDoc`: `setMemoryBudget(budget)`, `getMemoryBudget()`, `memoryStats()`, `reclaimMemory()`, `tryReclaimToBudget()`, and `tryCommit(options?)` (included in `decorateMethods` in `crates/loro-wasm/index.ts` so subscriber delivery runs consistently with `commit`).
- `UndoManager`: `new UndoManager(doc, { maxUndoBytes })`, `setMaxUndoBytes(bytes)`, `usedBytes()`.
- Memory limit rejections throw a structured JS `Error` with `name === "MemoryLimitExceededError"`, `code === "MemoryLimitExceeded"`, `kind` (`"ImportBlob" | "PendingPool" | "DocBudget"`), `usedBytes`, and `limitBytes`. `ImportBatchDetail` keeps plain `string` error fields (`error` / `rollbackError`).

## Undo accounting and checkpoints without container events

`UndoManager::record_new_checkpoint` can record same-peer imported history without
container events. Its empty `affected_cids` means unknown: remote diffs must be
kept conservatively, and merging a known span into that item keeps it unknown.
Otherwise undo may delete a remote prefix instead of the imported local text.

Finite byte-quota trimming calculates the two-stack total once, then subtracts
each removed item's allocation and any discarded row's capacity/remote diff.
It does not walk retained history for every eviction. The default unlimited
quota still performs no byte walk. Estimates retain the existing accounting
convention; this change does not redefine the reported undo byte fields.

## Structural Shrinkage & Native Out-of-Core Spilling

- **Lock-free unbudgeted fast paths (`LoroDocInner::budget_flags`)**: `LoroDoc::new()` injects `kv_block_cache` directly into `OpLog` / `DocState` without extra locking, and unbudgeted commits/imports short-circuit on `budget_flags` (`std::sync::atomic::AtomicU8`) before touching `memory_budget` or `txn` locks.
- **Compact `Op` (`<= 40B` on 64-bit, `op.rs`, `op/content.rs`, `list_op.rs`)**: `InnerContent::Map(Arc<MapSet>)` and compact `InnerListOp` variants (`Insert { slice, pos: u32 }`, `Set(Arc<SetListOp>)`, `StyleStart(Arc<StyleOp>)`, `Delete(CompactDeleteSpan)`) cut parsed op size without changing wire encoding.
- **Hybrid `ImVersionVector` (`version.rs`)**: Stores `<= 4` peers inline via `SmallVec<[(PeerID, Counter); 4]>` (`0` heap bytes) before promoting to `imbl::GenericHashMap` (`ImVvHeapMap`).
- **Sparse 64-slot paged `InnerStore` & lazy `ListState` child index (`inner_store.rs`, `list_state.rs`)**: `InnerStore` groups slots into 64-entry `StorePage`s whose `slots` grow only up to the highest populated offset in that page and free empty `Box<StorePage>` entries on cold demotion, value eviction, or rollback (`remove_created`). `ListState::child_container_to_index` stays `None` for scalar-only lists. `StrArena::shrink_spare_capacity()` runs on forced full reclamation (`reclaim_memory`).
- **Native out-of-core `FileKvStore` (`kv-store/src/file_store.rs`, `sstable.rs`, `MemoryBudget::spill_dir`)**: On `#[cfg(not(target_arch = "wasm32"))]`, `MemoryBudget::with_spill_dir(dir)` migrates both `OpLog` (`ChangeStore::replace_kv_backend`) and `DocState` (`ContainerStore::replace_kv_backend`, including `shallow_root_store`) onto `FileKvStore`. Flushed/imported/compacted SSTables write `.sst` backing files via `SpillFile` (positional reads served through `SharedBlockCache`, metadata-only `kv_bytes`, disk bytes tracked in `spilled_disk_bytes`, independent `.sst` copies on `Clone`/`fork()`, and automatic file unlinking on supersede, `Drop`, or switching `spill_dir` back to `None`). On `wasm32`, `spill_dir` is accepted on `MemoryBudget` as a no-op.
