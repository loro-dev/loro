# LoroDoc Bounded Memory Budget

Verified against code 2026-11-24.

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
  11. `undo_bytes`: stacks tracked by active `UndoManager` instances registered in `LoroDocInner::undo_managers`, including each item's recorded inverse with the values it keeps (`undo::CommitInverse::estimated_bytes`, `RecordedInverse::estimated_bytes`; [undo-log.md](undo-log.md)).
  12. `uncommitted_txn_bytes`: ops, `event_hints` capacity and the inverses recorded so far (`LocalInverseRecorder::Recording { bytes }`) in the currently open `Transaction`. When `memory_stats()` is called on the thread already holding `self.txn`, `self.txn.is_held_by_current_thread()` avoids self-deadlock and the caller supplies `uncommitted_txn_bytes` explicitly (e.g. `try_commit_with`).
- **`total_bytes`**: exact sum of the 12 RAM component fields above.
- **`spilled_disk_bytes`**: out-of-core spilled bytes (0 when using in-memory stores; intentionally excluded from `total_bytes`).

## Stage 1 & Stage 2 Soft Reclamation (Hysteresis, Clock Demotion & Anti-Thrashing)

- `effective_high_watermark()` defaults to 85% of `max_doc_bytes`; `effective_target_watermark()` defaults to 70% of `max_doc_bytes`.
- **Hysteresis**: when `total_bytes <= high`, commits skip soft reclamation (`enforce_watermarks_with_budget_locked`). Once `total_bytes > high`, reclamation aims down to `target`:
  - **Stage 1**: evicts decoded value wrappers (`evict_value_cache_to_bytes`), caps/clears `kv_block_cache`, frees `DiffCalculator` cold container trackers by LRU recency (`reclaim_cold_caches`, retaining the most recently used `keep_hot` trackers rather than the smallest), calls `reclaim_auxiliary_caches()` on `DocState` containers (including `RichtextState::id_to_cursor_cache`), compacts `OpLog` history (`OpLog::compact_history`, which also calls `ChangeStore::compact_parsed_blocks()`; skipped while `import_rollback` is active or `AppDag::has_pending_txn()` is true so `ChangeStore::flush_and_compact` never trips on uncommitted ops), frees `HistoryCache`, and trims registered `UndoManager`s.
  - **Stage 2**: if `total_bytes > target`, `InnerStore::demote_cold_states` encodes & drops non-empty, non-pinned materialized containers into `self.kv` and flushes KV memtables. Stage 2 skips redundant `oplog.flush_kv()` calls when Stage 1 already flushed `OpLog` and Stage 2 only mutated `DocState`. Newly created containers recorded in `created_journal_stack` are pinned while inside a rollback scope; empty containers and cleared deleted roots are skipped so they do not bloat KV.
  - **Soft vs Hard Demotion & Refault (Anti-Thrashing) Protection (`inner_store.rs`, `container_wrapper.rs`)**:
    - `InnerStore` tracks a monotonic `demote_epoch` (incremented once per actual `demote_cold_states` sweep, **not** per commit/op), current-op `access_epoch`, a bounded `recently_demoted` table (`ContainerIdx -> evicted_epoch`, FIFO cap `MAX_REFAULT_TRACKED = 512`), and a bounded `refault_protected` cooldown table (`ContainerIdx -> protected_until_epoch`, FIFO cap `512`).
    - When a demoted container is re-materialized from `self.kv` within `REFAULT_WINDOW_EPOCHS` (`4` demote sweeps), it is marked refault-protected for `REFAULT_COOLDOWN_EPOCHS` (`16` sweeps) and sets a refault signal. If a refault-protected container is still accessed in the current `access_epoch` during a soft sweep, its cooldown refreshes so sustained hot traffic does not periodically age out into eviction cycles.
    - **Known boundaries**:
      - Because `REFAULT_WINDOW_EPOCHS = 4` counts *demote sweeps* rather than commits/wall time, a container re-loaded after 5+ unrelated demote sweeps is treated as a fresh load rather than a refault.
      - Because `recently_demoted` and `refault_protected` are capped at `MAX_REFAULT_TRACKED = 512` entries (oldest evicted FIFO), workloads that simultaneously thrash `> 512` distinct hot containers in one cycle can overflow the tracking window (the 64-check `SoftBackoff` cap still bounds sweep frequency).
      - A single hot container larger than `high_watermark_bytes` stays materialized in soft mode once protected (causing `SoftBackoff` to arm until genuine growth or the 64-check probe), whereas exceeding hard `max_doc_bytes` deliberately bypasses soft protection & hysteresis so hard admission remains exact.
    - In **soft** mode (`hard_or_force = false`), `demote_cold_states` protects containers touched in the current `access_epoch` and active `refault_protected` containers across its three clock passes (second-chance clearing before evicting older unreferenced containers), returning `DemoteResult { reclaimed_bytes, blocked_by_protection, had_refaults }`. When protection/refaults prevent reaching `target`, `SoftBackoff` arms instead of resetting, stopping hot-container ping-pong.
    - In **hard/force** mode (`hard_or_force = true`, used by `reclaim_memory()` and hard-limit rescue), refault and current-epoch protections are bypassed and `recently_demoted` / `refault_protected` are cleared once demoted so full/hard reclamation remains deterministic. Stage 2 re-evaluates hard-limit status **after** Stage 1 (`over_doc_hard_after_stage1` / `over_global_hard_after_stage1`) so transient cache spikes cleared by Stage 1 do not force hard eviction of hot containers.
    - Tests: `hot_container_anti_thrashing_blocks_ping_pong_and_allows_force_full` (`loro.rs`) and `hot_container_anti_thrashing_preserves_data_and_reclaim_memory_still_demotes_all` (`crates/loro/tests/memory_budget.rs`).

## Runtime quota changes

`set_memory_budget` changes the hard document quota for the next transaction.
An already edited transaction keeps its captured `Transaction::doc_budget_limit`;
setting a quota in a commit callback never retroactively rejects those edits.
An empty auto-commit transaction can adopt the new quota before its first edit.
`try_commit` runs admission, rollback and commit under the same transaction lock;
manual commits and `apply_diff` use the same captured quota. Existing nested
batch scopes prevent a new budget scope from being inserted beneath them.
Single and batch imports perform soft reclamation after closing import rollback
scopes, including when only watermarks are configured. The cold-state
sweep uses a fixed starting clock hand so advancing the hand never skips slots.
Unbounded undo stacks skip byte estimation during trimming.

## Stage 3 Hard Admission & Atomic Rollback

- **`ImportBlob` (`max_import_blob_bytes`)**: checked pre-decode across `import*`, `import_batch*`, `import_batch_detailed*` (isolated per-blob `Failed`), and `import_json_updates` (via a zero-allocation `CountingWriter`).
- **`PendingPool` (`max_pending_bytes`)**: enforced incrementally during `PendingChanges::import_pending_change` with key/range deduplication; breaching it triggers mid-blob rollback in `OpLog`.
- **Hard-limit rescue & deferred `compact_history()` recovery**:
  - `enforce_watermarks_with_budget_locked` (`over_doc_hard`) and `GlobalMemoryPool::enforce_global_watermarks_for_active` (`over_global_hard`) **bypass `SoftBackoff`** whenever usage exceeds hard `max_doc_bytes`, `max_heap_bytes`, or WASM linear limits, escalating to `hard_or_force = true` before rejecting with `MemoryLimitExceeded`.
  - When `import_rollback` or `AppDag::has_pending_txn()` prevents `OpLog::compact_history()`, hard rescue also calls `OpLog::evict_flushed_history_cache()` (`ChangeStore::evict_flushed_cached_blocks`) to drop read-only already-flushed blocks loaded by `DiffCalculator`, and `enforce_watermarks_with_budget_locked` resets (does not arm) `self.soft_backoff` so post-rollback sweeps still run `compact_history()`.
  - Tests: `hard_limit_rescue_bypasses_soft_backoff_before_doc_oom`, `import_rollback_does_not_starve_post_import_compact_history` (`loro.rs`), and `hard_doc_budget_rescues_past_soft_backoff_on_commit_import_and_batch` (`crates/loro/tests/memory_budget.rs`).
- **`DocBudget` (`max_doc_bytes`)**:
  - **Remote imports & batches**: checked after `DocState` apply and watermark reclaim. Because `oplog.rollback_import(&mut state)` only reverts `OpLog`, callers (`import`, `BatchImportGuard::finish`, `init_snapshot_preserving_pending_with`) silently revert `DocState` first (`undo_import` / `revert_state_silently` + `take_events()` + `remove_created(&created)`), then roll back `OpLog`, and reset `DiffCalculator`. When `has_doc_budget` is set, `BatchImportGuard::finish` defers `doc.emit_events()` until after the `DocBudget` check succeeds so subscribers never observe rolled-back batches.
  - **`UndoManager`**: `compose_remote_event` filters out diffs for containers unrelated to `affected_cids` (while preserving `Tree` events) to prevent remote churn on untouched containers from bloating undo stacks, and `set_max_undo_bytes` / `shrink_to_fit` evict oldest items FIFO.
  - **`try_commit` / `try_commit_with` vs infallible `commit`**:
    - `commit()` / `commit_with()` stay infallible (performing Stage 1/2 best-effort reclamation without failing).
    - `try_commit()`, `try_commit_with()`, manual `Transaction::commit()`, and `apply_diff` enforce `max_doc_bytes`. Local transactions under `max_doc_bytes` arm `budget_base_scope` on `Transaction` (`begin_budget_rollback_scope` with `ignore_thread = true`).
    - On rollback (`finish_local_rollback`, `roll_back_scope`, `remove_created`), `LocalRollbackJournal` restores `SharedArena`'s `StrArena` to its checkpoint (`str_checkpoint`), `InnerStore::remove_created` truncates trailing empty slots and excess `Vec` capacity, `Transaction::roll_back_scope` shrinks/clears `event_hints`, and `rollback_transaction_scope_inner` calls `cleanup_rollback_transient_caches` to clear transient `HistoryCache`, `value_cache`, `kv_block_cache`, container auxiliary caches, flushed history blocks, and cold states when over budget so `DiffCalculator` history loads on a rejected commit do not block subsequent smaller commits.
    - **Hard-limit rejection tradeoff**: hard admission (`total_bytes > max_doc_bytes`) deliberately does not apply `SoftBackoff` hysteresis, so repeated caller loops that submit oversized `try_commit` transactions will run Stage 1 cleanup on each rejection. To avoid redundant Stage 2 work inside a single rejected commit, `enforce_watermarks_with_budget_locked` skips Stage 2 when `stats_after_stage1.total_bytes - state_bytes > max_doc_bytes` (demoting states cannot rescue the commit), avoids a second `demote_cold_states(0, ...)` pass when the first pass already targeted `0` or exhausted non-pinned states, and skips `state.flush_kv()` when `reclaimed_bytes == 0`. Tests: `rejected_commit_cleans_up_diff_calc_transient_caches`, `unrescuable_try_commit_skips_futile_stage2_demotion_before_rollback` (`loro.rs`), and `rejected_try_commit_cleans_transient_caches_so_later_commits_fit` (`crates/loro/tests/memory_budget.rs`).

## WASM / TypeScript Surface (`loro-crdt`)

- `LoroDoc`: `setMemoryBudget(budget)`, `getMemoryBudget()`, `memoryStats()`, `reclaimMemory()`, `tryReclaimToBudget()`, and `tryCommit(options?)` (included in `decorateMethods` in `crates/loro-wasm/index.ts` so subscriber delivery runs consistently with `commit`).
- `UndoManager`: `new UndoManager(doc, { maxUndoBytes })`, `setMaxUndoBytes(bytes)`, `usedBytes()`.
- Top-level global APIs: `setGlobalMemoryBudget(budget)`, `getGlobalMemoryBudget()`, `globalMemoryStats()`, `reclaimGlobalMemory(targetBytes?)`, and `tryReclaimGlobalToBudget()`.
- Memory limit rejections throw a structured JS `Error` with `name === "MemoryLimitExceededError"`, `code === "MemoryLimitExceeded"`, `kind` (`"ImportBlob" | "PendingPool" | "DocBudget" | "GlobalBudget"`), `usedBytes`, and `limitBytes`. `ImportBatchDetail` keeps plain `string` error fields (`error` / `rollbackError`).

## Process / WASM-Instance Global Memory Budget (`GlobalMemoryBudget`)

- **Registry & zero-Loom-noise fast path (`loro.rs`, `GLOBAL_POOL`, `GLOBAL_BUDGET_FLAGS`)**: Every `LoroDoc` assigns a `pool_doc_id` and stores a `Weak<LoroDocInner>` entry in `GlobalMemoryPool::docs`, automatically removing itself in `Drop for LoroDocInner`. Under `#[cfg(loom)]`, unbudgeted docs use `pool_doc_id = 0` so Loom explores zero extra pool locks when global budgets are unset.
- **Global `SharedBlockCache` parent delegation (`kv-store/src/sstable.rs`)**: Enabling global memory management attaches `GlobalMemoryPool::global_block_cache` (`16 MiB` default, or `<= max_heap_bytes / 8`) as parent on every `LoroDoc`'s `kv_block_cache`. Because `NEXT_SSTABLE_ID` is process-wide, keys never collide, and `local_used_bytes()` reports `0` on delegating docs so `DocMemoryStats` never double-counts global cache bytes. Explicit per-doc `max_kv_block_cache_bytes` opts that document out into its own local cache.
- **Cross-doc non-blocking LRU reclamation (`LoroMutex::try_lock` in `lock.rs` / `sync.rs`)**: `last_access_tick` (`GLOBAL_ACCESS_TICK`) advances on mutations, imports, checkouts, exports, and container/value reads whenever a global budget is active. When global usage breaches `effective_high_watermark()` (default 85% of `max_heap_bytes`, or 80% of `max_wasm_linear_bytes` when WASM pages near limit), `reclaim_idle_docs_to_target` clears the global block cache, skips any doc whose `Txn`/`OpLog`/`DocState` is busy, dirty (`!txn.is_empty()`), or inside an import rollback scope, and sweeps idle docs oldest-tick-first through Stage 1 (caches/compact/flush/arena shrink) then Stage 2 (cold state demotion) before falling back to the active doc.
- **WASM live heap + linear memory tracking & pre-malloc admission (`crates/loro-internal/src/memory.rs`, `crates/loro-wasm/src/lib.rs`)**:
  - `ReportingAlloc` wires `enable_global_heap_tracker()`, `record_global_alloc`, `record_global_dealloc`, and `record_global_realloc` with relaxed atomics (avoiding `crate::sync` Loom instrumentation), exposing live/peak Rust heap bytes alongside `wasm_linear_memory_bytes()` (`wasm32::memory_size(0) * 65536` in `u64` so 4 GiB never overflows 32-bit `usize`).
  - `LoroDoc::import`, `importBatch`, and `importBatchDetailed` read JS `Uint8Array.byteLength` and run `preflight_external_import_alloc` **before** `.to_vec()` allocates in WASM linear memory, proactively reclaiming idle docs and rejecting with `MemoryLimitKind::GlobalBudget` before V8's `wasm-max-mem-pages` / 4 GiB ceiling traps.
  - Large JS strings (`>= 64 KiB` code units) passed to `LoroText` (`insert`, `insertUtf8`, `splice`, `update`, `updateByLine`) check `preflight_wasm_linear_alloc(code_units * 3)` before `wasm-bindgen` copies the string onto the WASM heap when `maxWasmLinearBytes` is configured (honoring configured watermarks and `SoftBackoff` under soft pressure, and rescuing before rejecting with `GlobalBudget`), keeping moderate edits eligible for `tryCommit` rollback while blocking multi-MiB linear-memory traps early.
  - Because `memory.grow` never shrinks on `wasm32`, `check_global_budget_with_active` compares `allocated_heap_bytes > max_heap_bytes`, and allows writes after `wasm_linear_memory_bytes` touched `max_wasm_linear_bytes` only if `lin_now <= linear_before` (0 new WASM pages) and live heap dropped `<= 70%` of `max_wasm_linear_bytes` after idle-doc reclamation.

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

## Page statistics cache

`StorePage::state_bytes` caches the sum for at most 64 materialized containers.
Every mutable entry lookup, insertion, removal, mutable iteration, flush and cold
state demotion invalidates that page. Read-only statistics reuse clean pages.
`estimated_state_bytes` still sums page/table overhead and never postpones a dirty
page refresh past hard admission. Debug builds independently walk all entries and
assert cached and fresh totals agree. The work-count regression checks 20 edits
among 512 containers cause exactly 20 page recomputations. Complexity is page
count plus containers in dirty pages, rather than strictly constant time.

## Global pressure fast path and recovery

When exact heap tracking is active (WASM), a document with the default local
memory policy skips per-document soft-reclamation statistics below the global
high watermark. Explicit statistics and hard admission still run their own
checks. Local quotas/caches/undo policies and forced reclamation bypass this
shortcut. Native builds without heap tracking retain the estimate-based path.

Linear memory above 80% of its ceiling triggers extra reclamation only when live
heap also exceeds 70% of that ceiling. Reclamation then aims at an absolute target
(no more than 65% of the linear ceiling), rather than 70% of each commit's current
usage. Thus freeing heap can end pressure while WASM pages remain allocated.
A runtime regression verifies repeated commits preserve idle materialized state
once heap has fallen, and a pure threshold test covers pressure returning.

The global quota setter now avoids inserting rollback scopes beneath a batch or
into a transaction that already accepted edits; the next transaction picks up
new global hard admission. Local quota snapshots and global linear baselines are
both retained across the stacked commit paths.

## Structural Shrinkage, Incremental L0 Spilling & Native Out-of-Core Storage

- **Lock-free unbudgeted fast paths (`LoroDocInner::budget_flags`)**: `LoroDoc::new()` injects `kv_block_cache` directly into `OpLog` / `DocState` without extra locking, and unbudgeted commits/imports short-circuit on `budget_flags` (`std::sync::atomic::AtomicU8`) before touching `memory_budget` or `txn` locks.
- **Compact `Op` (`<= 40B` on 64-bit, `op.rs`, `op/content.rs`, `list_op.rs`)**: `InnerContent::Map(Arc<MapSet>)` and compact `InnerListOp` variants (`Insert { slice, pos: u32 }`, `Set(Arc<SetListOp>)`, `StyleStart(Arc<StyleOp>)`, `Delete(CompactDeleteSpan)`) cut parsed op size without changing wire encoding.
- **Hybrid `ImVersionVector` (`version.rs`)**: Stores `<= 4` peers inline via `SmallVec<[(PeerID, Counter); 4]>` (`0` heap bytes) before promoting to `imbl::GenericHashMap` (`ImVvHeapMap`).
- **Sparse 64-slot paged `InnerStore` & lazy `ListState` child index (`inner_store.rs`, `list_state.rs`)**: `InnerStore` groups slots into 64-entry `StorePage`s whose `slots` grow only up to the highest populated offset in that page and free empty `Box<StorePage>` entries on cold demotion, value eviction, or rollback (`remove_created`). `ListState::child_container_to_index` stays `None` for scalar-only lists. `StrArena::shrink_spare_capacity()` runs on forced full reclamation (`reclaim_memory`).
- **Incremental L0 KV flush, size-tiered compaction & L0 Bloom filters (`kv-store/src/{mem_store.rs, file_store.rs, sstable.rs}`)**: `MemKvStore::flush()` writes only `mem_table` into a new L0 `SsTable` (retaining empty deletion tombstones whenever older SSTables exist) instead of calling `export_all()` to rewrite the entire store on every flush; callers of the public `loro-kv-store` `MemKvStore::flush()` / `FileKvStore::flush()` API may therefore retain multiple SSTables until `export_all()` or full compaction. `maybe_compact_sstables()` / `compact_sstables_from()` trigger bounded suffix compactions when `suffix_bytes >= table[i].data_size() / 2` or `ss_table.len() > MAX_SS_TABLES` (`6`), using `SsTableIter::new_scan_uncached` so compaction scans do not pollute `SharedBlockCache` (and stripping tombstones only when compacting from `start == 0` with `!should_encode_none`). To prevent multi-table point-read amplification (`get` / `contains_key`), L0/delta SSTables (tables above index `0`) attach an in-memory-only 10-bit/key `BloomFilter` (no wire format change, accounted in `SsTable::estimated_memory_bytes()`), keeping cold point reads across 6 interleaved tables near 1x (`bloom_filters_bound_point_read_amplification_across_interleaved_l0_tables`). `FileKvStore::flush()` calls `spill_tables()` on the newly appended/compacted SSTables (reusing already-spilled `.sst` files).
- **Native out-of-core `FileKvStore` (`kv-store/src/file_store.rs`, `sstable.rs`, `MemoryBudget::spill_dir`)**: On `#[cfg(not(target_arch = "wasm32"))]`, `MemoryBudget::with_spill_dir(dir)` migrates both `OpLog` (`ChangeStore::replace_kv_backend`) and `DocState` (`ContainerStore::replace_kv_backend`, including `shallow_root_store`) onto `FileKvStore`. Flushed/imported/compacted SSTables write `.sst` backing files via `SpillFile` (positional reads served through `SharedBlockCache`, metadata-only `kv_bytes`, disk bytes tracked in `spilled_disk_bytes`, shared immutable `.sst` files on `Clone`/`fork()` (separate mutable buffers/cache, file removed after the last owner), Unix owner-only `0600` files, and automatic file unlinking after the last owner releases a superseded/dropped file). On `wasm32`, `spill_dir` is accepted on `MemoryBudget` as a no-op.

## Fork and shallow-root backend inheritance

`loro.rs::fork` and `fork.rs::fork_at_inner` install an independent clone of the
full local `MemoryBudget` on the new document before decoding its state/history.
This includes quotas, cache limits, watermarks and the native spill directory.
Changing the fork's policy does not change its source. Internal snapshot handoff
still bypasses the external import-blob cap; subsequent public imports enforce it.

`ContainerStore::decode_gc` creates the shallow-root `InnerStore` with
`new_empty_same_backend`, so configuring spill before a shallow import applies
to both root and latest stores.

## Shallow root input buffer

Verified against code 2026-11-16.

Snapshot sections are zero-copy slices of the import input. The shallow root
keeps `encoded_state_bytes` (and its store) for the document's lifetime, and the
latest state store keeps slices of the state section, so a shallow import used
to keep the whole input allocated, op log section included, after the op log
was rewritten or spilled; statistics counted only the kept sections. Reproduced
by `rewritten_stores_release_the_snapshot_input_buffer` (`fast_snapshot.rs`): a
full snapshot's input was released after an edit and `reclaim_memory`, a shallow
one's was not. Both the shallow and state sections had to be copied (either copy
alone still kept the input). `decode_snapshot_inner` now copies them
(`copy_shallow_sections`) unless the op log section is under an eighth of them,
keeping `from_snapshot_bytes` zero-copy where little could be pinned. Measured
(`crates/loro/examples/shallow_import_bench.rs`, 197 KB shallow snapshot): heap
left after an edit and `reclaim_memory` 1,270,822 → 1,181,430 bytes, import peak
and time unchanged within noise. `pruned_root`'s cache is separate and untested.

## Reclamation sweep and retained handler reads

A sweep first checks current usage against its target before clearing shared
blocks. It records which documents completed Stage 1; Stage 2 skips that work
for those documents, while retrying Stage 1 for any document that was busy on
the first pass. Each stage retains its try-lock/dirty-transaction guards.
`BasicHandler` and `HandlerTrait` state/value read entry points refresh the
owning document's access tick before taking the state lock. A retained Map read
therefore keeps its document recent even without a new `get_map` call.

Small `importBatchDetailed` calls retain their already-copied owned buffers
through metadata ordering and import. `import_batch_detailed_with_blob_results`
accepts an exact-size iterator, avoiding an extra result vector on the existing
Rust owned-bytes path. Preflight errors stay per-blob errors, including the
single-blob special case. Large batches still use the two-pass streaming path.

## Last handle and registry references

Verified against code 2026-11-18.

`Drop for LoroDoc` (`lib.rs`) commits pending edits only when it releases the last
user handle. The global registry (`GlobalMemoryPool::snapshot_live_docs`, `loro.rs`)
holds temporary `Arc<LoroDocInner>`s; a plain `strong_count == 1` check made the
last handle skip its commit while a scan held one, and the edits were lost when
the registry released it (reproduced: about half of 20,000 drops with a thread
calling `global_memory_stats`). `LoroDocInner::user_handles` (a lock-free
`AtomicUsize` counting only live user-facing `LoroDoc` and attached container
handles) fixes it without locking a drop-gate mutex on every handle drop:

- `LoroDoc::new` initializes `user_handles` to `1` (`from_new_inner`);
  `LoroDoc::clone` and `LoroDoc::from_inner` increment it; internal borrowed
  views (`LoroDoc::borrow_from_inner`) and `RegistryRef::upgrade` never touch it.
- `Drop for LoroDoc` decrements `user_handles` via `compare_exchange` when `> 1`
  and releases its `ManuallyDrop<Arc<LoroDocInner>>` immediately. When `1`, it
  runs `implicit_commit_then_stop()` while keeping `user_handles == 1` (so any
  temporary event handlers created during post-commit emission take the `> 1`
  fast path rather than nesting a second final commit, and a subscriber that
  retains a cloned handle causes the `1 -> 0` CAS to retry as a non-final drop).
- The registry never commits. Its `RegistryRef` holds only `Arc<LoroDocInner>`,
  and `snapshot_live_docs` upgrades `Weak<LoroDocInner>` entries under the
  registry lock without per-document gate allocations.
- Registry code must not create a counted `LoroDoc` handle from a registry
  reference (`LoroDocInner::arm_budget_scope_on_txn` exists for that reason).

Tests: `reclamation_work_tests` in `loro.rs`, two threaded tests in
`crates/loro/tests/global_memory_budget.rs`, concurrent clone/drop tests in
`crates/loro/tests/hot_path_regressions.rs`, and the Loom models in
`crates/loro/tests/global_registry_loom.rs` (part of `pnpm test-loom`).

## Boxed op payloads

`InnerContent::payload_heap_bytes` (`op/content.rs`) estimates heap owned beyond
`size_of::<Op>()`: `Arc<MapSet>`, `Arc<SetListOp>`, `Arc<StyleOp>`, `Arc<TreeOp>`,
future values, and list-insert values. It is conservative: a value shared with
state, or a parsed block's value buffer, is counted for each op that holds it.
`Transaction` keeps the sum in `local_ops_payload_bytes` (pushed ops, recomputed
from the kept ops in `roll_back_scope`). `ChangesBlock::parsed_bytes` caches the
parsed-change total per block; every content mutation goes through
`ChangesBlock::content_mut` (or invalidates directly in `push_change`), so a
statistics read walks only blocks changed since the previous read. Before this,
twenty 512 KiB overwrites of one key in a transaction were estimated at a few KiB
and `try_commit` accepted them under a 2 MiB quota.

## Spill directory switches

`LoroDoc::set_memory_budget_inner` (`loro.rs`) moves the stores before it records
a new `spill_dir`. `switch_spill_backend` builds both backends, then under the op
log and state locks fills and flushes them (`ChangeStore::prepare_kv_backend`,
`ContainerStore::prepare_kv_backend` including the shallow root, both scanning
straight into the new store instead of collecting a `Vec`) and installs both only
if neither reported a write failure (`KvStore::take_spill_error`, recorded by
`FileKvStore::spill_tables`). On failure the current stores stay; the infallible
setter logs it, keeps the previous `spill_dir` (so the same directory can be set
again to retry) and applies the other fields; `try_set_memory_budget` returns the
error and changes nothing. A switch only writes what the stores hold encoded at
that moment; a later flush that cannot write keeps its tables in RAM, logs, and
records the error for `LoroDoc::take_spill_error` (Rust only; spilling is a no-op
on WASM).
Tests: `crates/loro/tests/out_of_core.rs` (a file at the path; `/proc/self` as a
directory that rejects writes, Linux only).

## Import rollback without reading history

Verified against code 2026-11-17.

An import that a hard quota (`max_doc_bytes` or a global one) may reject
applies its diff to the state, then checks the quota. A rejected import used
to restore the state by computing the inverse diff from history
(`revert_state_silently`). If that read failed (a corrupt block), a single
import returned an error with state and history left at the new version and
its import scope open; a batch rolled history back but not state. Computing the
inverse before applying was correct but cost 3.8 -> 18 ms for a 20k-op import
and about 250x for small updates into a large text.

Now the state records an undo log (`state/undo_log.rs`) while such an import
applies: `DocState::begin_import_undo`, `take_import_undo`, `undo_import`. Before
`DocState::apply_diff` applies a container's diff it asks the container for a
`StateUndo` (`ContainerState::undo_record`), read from the current state:

| State | Record |
| --- | --- |
| Map | each key's previous `MapValue` (or absent) |
| List | inserted ranges; deleted elements with their ids |
| Rich text | inserted entity ranges; deleted chunks (`chunks_in_entity_range`), reinserted through `apply_diff` so style ranges are rebuilt |
| Tree | each node's previous `TreeStateNode` (or absent); moving back with its old `last_move_op` restores sibling order |
| Movable list | the inverse of each `InnerState` primitive (list item insert/remove, `pointed_by`, element, child map), recorded while applying (`finish_undo_record`) |
| Counter | the previous value |

`undo_import` replays the records newest first, then restores the frontiers and
clears the liveness caches; callers then remove created containers (created
journal) and roll back the op log as before. It reads no history and cannot
fail. Recording costs about as much as the part of the state the diff changes,
and nothing is recorded without a hard quota. All three paths use it:
`update_oplog_and_apply_delta_to_state_if_needed`,
`import_changes_and_apply_delta_to_state_if_needed`, and `BatchImportGuard::finish`
(around the reattach checkout). A new kind of state change in `apply_diff` or a
new state type must extend its record, or rollback is inexact.

Tests (`src/tests/import_undo_log.rs`): a differential test that rejects random
imports (single and batch) of concurrent edits to every container type and
compares the document with a fork taken before the import, including after the
same later edits and imports; and `rolling_back_a_rejected_import_computes_no_diff`.

## Spill IO errors

Verified against code 2026-11-16.

- **Write failures** after a switch keep the tables in RAM (no data loss).
  `LoroDoc::take_spill_error()` returns the first one since the previous call
  (op log and state stores joined with `"; "`), then `None`. Test:
  `a_flush_that_cannot_write_is_reported_by_take_spill_error` (`out_of_core.rs`).
- **Read failures** (`SsTable::read_block`, `with_raw_block`, `export_all` on a
  file backing, `kv-store/src/sstable.rs`) still panic. The `KvStore` read
  interface has no error type, and most callers sit under APIs without one.
  `SpillFile` keeps its handle open and reads with `pread`, so on Unix a deleted
  spill file stays readable.
- **Checksums**: blocks read back from a spill file are re-verified with xxh32
  (`spilled_block_checksum_ok`; `export_all` checks every block). A mismatch
  panics with "failed its checksum" instead of decoding wrong data. Before, the
  bytes went straight to `Block::decode`. Test:
  `a_corrupted_spill_file_fails_its_checksum_instead_of_returning_wrong_data`
  (`file_store.rs`).

## Spill directory without a budget

A `spill_dir` only chooses where flushed tables live. Without watermarks or
`max_doc_bytes`, nothing flushes automatically: data moves to disk on
`reclaim_memory` (or a global sweep), and `set_memory_budget` logs a warning.
Set watermarks for automatic reclamation. There is no implicit default policy.

## WASM linear memory ceiling

Verified against code 2026-11-16.

WASM pages never shrink. Hard admission (`check_global_budget_with_active`)
accepts a commit past `max_wasm_linear_bytes` if it added no pages and the live
heap is at most 70% of the ceiling (`heap_fits_existing_pages`).
`try_reclaim_global_to_budget` now uses the same heap rule
(`linear_budget_satisfied`); before, it failed forever once the pages had grown,
even while commits succeeded. `wasm_linear_memory_exceeded()` (Rust) and the JS
`GlobalMemoryStats.wasmLinearMemoryExceeded` report whether the pages are past
the ceiling, so an application can recreate the instance. `GlobalMemoryStats`
gained no Rust field (it is not `#[non_exhaustive]`).

## Global soft-sweep backoff & Preflight Batching

`GlobalMemoryPool::enforce_global_watermarks_for_active` falls back to Stage 1
and Stage 2 on the active document when idle documents cannot bring usage to the
target. When even that fails (live state or refault-protected containers exceed
the target), `record_soft_sweep_result` stores the usage left by the failed sweep,
and `soft_reclaim_backed_off` skips later soft sweeps until usage exceeds that by
`max(floor / 8, 256 KiB)` or `SOFT_BACKOFF_MAX_SKIPS` (64) checks were skipped,
so documents that become idle are still reclaimed without growth. Usage below
the high watermark, a successful sweep, or `set_global_memory_budget` clears the
backoff.

- **Unified preflight backoff & single batch preflight (`loro.rs`)**:
  - `preflight_global_watermark_for_import` (called by `preflight_import_bytes`) and
    `preflight_wasm_linear_alloc` consult `pool.soft_backoff` for soft pressure
    (`projected <= hard_cap`) and update `record_soft_sweep_result` after running.
    Whenever `projected > cap` (both `include_buffer_alloc == true` pre-copy
    preflights and `include_buffer_alloc == false` `import` / `import_batch`
    preflights, as well as WASM linear ceiling preflights), they **bypass
    `soft_backoff`** and run a hard-rescue sweep (`hard_demote = true`); only
    pre-copy/linear-alloc preflights (`include_buffer_alloc == true` /
    `max_wasm_linear_bytes`) return `GlobalBudget` before decode/copy.
  - `import_batch*` / `import_blobs_as_batch` run the global preflight watermark
    check at most **once** at batch entry (using total/max blob size) while
    keeping O(1) `check_import_blob_size_only` checks inside the per-blob decode
    and import loops.
  - Hard admission (`check_global_budget_with_active`) and hard-limit rescue
    (`is_over_global_hard_limit`) never stay blocked by `soft_backoff`.
- **Stage 1 cache-rebuild thrash detection (`SoftBackoff::backed_off_with_cache`)**:
  - Both per-doc (`LoroDocInner::soft_backoff`) and global (`GlobalMemoryPool::soft_backoff`,
    covering `enforce_global_watermarks_for_active`, `preflight_global_watermark_for_import`,
    and `preflight_wasm_linear_alloc`) record the pre-sweep footprint and cleared Stage 1
    cache bytes (`block_cache + value_cache + change_cache + history_cache + diff_calc`, or
    `global_block_cache` on global preflights).
  - Detection operates **between consecutive soft sweeps** without a fixed check-count expiry
    window: whenever the next crossing above the high watermark has rebuilt `>= 50%` of the
    last sweep's cleared cache bytes while both total usage (`used <= pre_sweep +
    max(pre_sweep/8, 256 KiB)`) and non-cache usage (`cur_non_cache <= pre_non_cache +
    max(pre_non_cache/8, 256 KiB)`) show no genuine non-cache growth, `SoftBackoff` arms at
    the rebuilt footprint regardless of how many below-high checks occurred in between (so
    both every-op reads and sparse periodic reads like every 6 or 20 imports stop clearing
    and re-decompressing `block_cache` on every read).
  - Genuine non-cache growth bypasses cache-rebuild backoff and triggers an immediate sweep;
    hard-limit rescues and explicit `reclaim_memory()` / `reclaim_global_memory(None)` /
    budget setters reset `SoftBackoff`; and the periodic probe after `SOFT_BACKOFF_MAX_SKIPS`
    (`64`) checks still reclaims `block_cache` once reads stop.
- Tests: `a_failed_global_soft_sweep_backs_off_until_usage_grows`,
  `preflight_respects_soft_backoff_and_rescues_before_global_oom`,
  `import_preflight_without_buffer_alloc_bypasses_backoff_when_projected_exceeds_cap`,
  `batch_import_runs_single_global_preflight`,
  `stage1_block_cache_rebuild_thrashing_enters_soft_backoff`,
  `stage1_sparse_periodic_reads_thrash_backoff_and_global_consistency` (`loro.rs`), and
  `global_preflight_backoff_and_hard_rescue_on_import_and_batch`
  (`crates/loro/tests/global_memory_budget.rs`).

The per-document sweep (`enforce_watermarks_with_budget_locked`) uses the same
`SoftBackoff` (`LoroDocInner::soft_backoff`): a soft sweep that ends above the target
(including when hot/refault-protected containers are kept materialized) records
the usage it left, and later commits skip Stage 1/2 until usage grows
by the same margin or 64 checks were skipped. Usage at or below the high
watermark and `set_memory_budget` clear it; `reclaim_memory` (`force_full`)
and hard-limit rescue (`total_bytes > max_doc_bytes`) never back off, and an
active `import_rollback` scope resets backoff instead of starving the
post-rollback `compact_history()`. Test:
`a_failed_document_soft_sweep_backs_off_until_usage_grows`.

## Incremental native global estimate

Verified against code 2026-11-16.

Without an allocator tracker (native), the global estimate is the sum of each
document's `cached_doc_bytes` (`GlobalMemoryPool::total_cached_doc_bytes`,
spilled bytes in `total_cached_spilled_bytes`) plus the global block cache.
`GlobalMemoryPool::estimated_docs_total` first recomputes only the documents in
`dirty_docs` (`refresh_dirty_docs`); hard admission, the watermark check,
`global_memory_stats` and `reclaim_global_memory` all go through it. Before, each
budgeted commit recomputed every live document (1,000 idle documents: ~880 µs per
`try_commit`, ~300 µs per `commit`; now ~3 µs and ~5 µs, flat in the number of
documents; `crates/loro/examples/global_budget_scaling_bench.rs`).

`DocStatsDirty` (`loro.rs`) is the per-document flag. The rule: anything that can
change a `DocMemoryStats` component marks the document while it holds the lock
that component is read under, or after the change. The refresh
(`refresh_cached_doc_bytes_for_inner`) takes the `Txn`, `OpLog`, `DocState` and
`DiffCalculator` locks without marking (`LoroMutex::try_lock_unmarked`), clears
the flag, then reads, so a concurrent change is either seen or marks again.
Mark sources:

- every `lock`/`try_lock` of those four locks (`LoroMutex::set_stats_dirty`);
- arena writes (`SharedArena::containers_write`/`str_write`): handlers register
  containers without the document's locks;
- `ChangeStore::load_parsed_block`/`parse_cached_block` (the creator resolver
  parses history without the op log lock);
- undo managers: `MarkedUndoGuard` marks before releasing the manager's lock;
  `StatsMarkOnDrop` marks after the manager and its subscriptions drop.

A new state that changes outside these locks must add a mark, or the estimate
undercounts. Debug builds check it: `debug_assert_clean_caches_fresh` compares
every clean, idle document's cache with a fresh computation on each estimate
(O(documents), debug only). A document starts dirty and queued by `register`;
`unregister` removes it, so `dirty_docs` holds live documents only, also without
a global budget. A busy document (or undo manager) keeps its cached value and
stays queued; the active document is passed in by its caller and stays queued.
Tests: `the_global_estimate_refreshes_only_documents_marked_dirty` (`loro.rs`),
`global_estimate_stays_exact_after_reads_edits_undo_drops_and_policy_switches`
(`crates/loro/tests/global_memory_budget.rs`).

## String arena shrink

Verified against code 2026-11-16.

`StrArena::shrink_spare_capacity` copies the arena into an exact-size buffer;
`BytesSlice`s into the old buffer keep it alive, and `arena_bytes` counts length,
not capacity. Measured (`crates/loro/examples/arena_shrink_bench.rs`, HEAD vs HEAD
with the shrink disabled, live heap after `reclaim_memory`): 30k local text
inserts 1,057,381 → 987,551 bytes (about 70 KB freed, 217 KB more allocated
during the call); local map strings and imported text unchanged. It does free
spare capacity, so it stays in `reclaim_memory` and in the global fallback (which
now backs off).

## Remaining work

The state undo log and recorded UndoManager inverses are documented above and in
[undo-log.md](undo-log.md). Remaining optimization and validation work:

- File-backed KV flush now appends L0 SSTables incrementally and compacts
  suffixes by size-tiered/count thresholds (`MAX_SS_TABLES = 6`), avoiding
  full-table rewrites on routine flushes; full compaction/rewrite happens only
  via `export_all` or when a suffix compaction reaches index `0`. Background
  non-blocking compaction remains future work if needed.
- WASM `insert`, `insertUtf8` and `splice` encode strings up to 42 UTF-16 code
  units in a 128-byte Rust stack buffer. Empty strings and single code units
  skip the encoding view entirely. Larger strings and environments
  without `TextEncoder.encodeInto` go through the original checked conversion.
  Large `importBatchDetailed` calls retain their streaming preflights.
- The small-oplog shallow-snapshot threshold intentionally retains its input;
  `pruned_root` retention has not been measured independently.
- Full interleaved A/B coverage against main for JSON, checkout and unbudgeted
  workloads remains separate from functional release validation. Raw repair
  measurements are retained in `benchmarks/memory-budget-repair-20261116/`.
- Boxed payload accounting is conservative; the suspected overestimate on large
  text documents has not been established by an allocator comparison.

## Concurrent statistics verification

Verified against code 2026-11-17.

`fresh_doc_stats_unmarked` compares a clean document's cached total against a
fresh total while still holding its transaction, op log, state and diff locks.
Releasing those locks before loading the cached total lets a writer and another
refresh replace the cache between the two reads, causing a false debug panic.
`last_handle_drop_commits_while_another_thread_scans_the_registry` exercises this
concurrent statistics path. The comparison does not change release accounting.
