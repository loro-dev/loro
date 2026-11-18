# Round 2 hot-path and complexity optimizations

Measured on 2026-11-18 on Tonbo `loro`, Linux ARM64, Rust 1.100.0.

## Summary of fixes and optimizations

1. **Lock-free `LoroDocInner::user_handles` replacing `DocDropGate` (`lib.rs`, `loro.rs`)**:
   `Drop for LoroDoc` previously cloned, locked, and dropped a `DocDropGate` mutex on every `LoroDoc` and handler drop, while `Transaction::_commit` temporarily constructed and dropped a `LoroDoc` handle. Counting live user-facing handles with an atomic `user_handles` (while internal borrowed views use `LoroDoc::borrow_from_inner` and `RegistryRef` upgrades `Weak<LoroDocInner>` directly) removes per-drop mutex traffic while preserving final-handle implicit commits during concurrent registry scans.
2. **Single-registration handler creation (`loro.rs`, `handler.rs`, `arena.rs`, `state.rs`)**:
   `LoroDoc::resolve_and_ensure_container` registers a container in `SharedArena` once and constructs `Handler::new_attached_with_idx` directly instead of registering twice (`into_container_id` + `Handler::new_attached`), and `SharedArena::register_container` marks `stats_dirty` only when a new container is inserted.
3. **Allocation-free transaction commit & single-container tracking (`txn.rs`, `state.rs`, `subscription.rs`, `utils/subscription.rs`)**:
   - Replaced the boxed per-transaction `on_commit` closure with `emit_on_commit: bool` + `LoroDoc::emit_post_commit`.
   - Added lock-free `SubscriberSet::is_empty_fast()` and `Observer::has_container_subs`, allowing `_commit` with no `pre_commit` subscribers to skip `ChangeModifier` allocation and collapse three `OpLog` lock acquisitions into one.
   - Replaced `DocState::changed_idx_in_txn: FxHashSet<ContainerIdx>` with `ChangedIdxSet` (`first: Option<ContainerIdx>` inline before spilling to `FxHashSet`), eliminating hash-set allocation and table clearing on single-container transactions.
   - Replaced `LoroDocInner::undo_manager_registered` (monotonic) with `active_undo_managers: AtomicUsize` decremented on `UndoManager` drop, and added `LocalInverseRecorder::SingleCompact` so `Transaction` avoids allocating `Vec<RecordedOp>` for single/coalesced text and list insertions.
4. **Compact list insert inverses, in-place grouped coalescing, and $O(1)$ undo stack byte accounting (`txn.rs`, `undo.rs`)**:
   - Generalized compact insertion inverses (`CommitInverse::compact_insert`) to cover `LoroList` as well as `LoroText`, and coalesced adjacent insertions in place inside `LocalInverse::chain`.
   - Cached `commits_bytes` on `LocalInverse` and incremental `cached_bytes: Option<usize>` on `undo::Stack`, fixing an $O(N^2)$ stack walk on every commit when `max_undo_bytes` is configured.
   - Made `Stack::clear()` a zero-allocation no-op when `redo_stack` is already empty, and added `Stack::try_merge_top_from_events` to avoid per-commit `FxHashSet<ContainerID>` allocation during grouped edits.
5. **$O(1)$ warm value-cache read hits and preserved page state statistics (`container_wrapper.rs`, `inner_store.rs`, `richtext_state.rs`)**:
   - Cached `LazyDecodedValue::cached_bytes` (`AtomicUsize`, invalidated only if `Map`'s `OnceCell<LoroValue>` initializes) and added `ContainerWrapper::try_mark_value_cache_hit`, removing an $O(N)$ walk of decoded maps/lists and `StorePage::cached_state_bytes` invalidation on every warm snapshot-backed read hit.
   - Preserved `StorePage::cached_state_bytes` in `InnerStore::with_container_for_read` when `entry.is_fully_materialized_state()` holds.
6. **Multi-leaf and left-boundary deletion coalescing in `DeltaRope` (`crates/delta/src/delta_rope/compose.rs`)**:
   - Added `_delete_batch_leaves` and offset-zero left-neighbour absorption so repeated fixed-offset forward deletions collapse zero-output deletion fragments instead of accumulating leaves.

## Protocol

All three variants were freshly built in the same machine activation with Rust 1.100.0 from clean source trees:

- `prestack`: `b005024cbd896e394cc875699a0c7f6cfa272a9e` (before the memory-budget and recorded-inverse stack).
- `pr1314`: `77e3fa2f` (PR #1314 after review fixes).
- `v2`: this revision.

Processes were pinned to CPU 0 (`os.sched_setaffinity`) with no concurrent compilation or tests. Raw invocations, exit codes, binary SHA-256 digests, and medians are recorded in `v2-measurements.jsonl`, `v2-summary.json`, `v2-medians.json`, and `v2-binary-sha256.json`.

## Results (medians in ms)

### `perf_hot_paths` & `perf_followup` (12 interleaved samples per cell after warmup)

| Scenario | Pre-stack (`b005024c`) | Prior PR #1314 (`77e3fa2f`) | Optimized (`v2`) | `v2` vs `pr1314` | `v2` vs `prestack` |
| --- | ---: | ---: | ---: | ---: | ---: |
| Text, 20k separate commits, no undo | 33.425 | 35.919 | **25.980** | -27.67% | **-22.27%** |
| Map, 2k small commits after 10k-container setup | 3.248 | 3.593 | **2.721** | -24.27% | **-16.23%** |
| Text, 20k separate commits, undo enabled | 64.715 | 74.136 | **61.627** | -16.87% | **-4.77%** |
| Root handler lookup, 100k | 13.308 | 16.560 | **11.379** | -31.29% | **-14.50%** |
| Child handler lookup, 100k | 15.697 | 17.493 | **16.435** | -6.05% | +4.70% |
| JSONPath, 1k scans over 2k live entries | 406.519 | 391.925 | **402.017** | +2.57% | **-1.11%** |
| Pending, ten reversed-update imports | 450.474 | 468.163 | **461.377** | -1.45% | +2.42% |
| Map, ten batches of 10k edits | 63.324 | 62.787 | **63.187** | +0.64% | -0.22% |
| Text, 20k inserts in one txn + first undo | 8.890 | 10.188 | **9.838** | -3.44% | +10.66% |
| Text, 20k grouped commits + first undo | 65.199 | 80.484 | **61.826** | -23.18% | **-5.17%** |

### Grouped 20,000-commit edit + undo + redo (`review-grouped.rs`, 6 interleaved samples per cell)

| Workload (20,000 grouped commits) | Pre-stack total (edit / undo / redo) | Prior PR #1314 total (edit / undo / redo) | Optimized `v2` total (edit / undo / redo) | `v2` vs `pr1314` |
| --- | ---: | ---: | ---: | ---: |
| Continuous text inserts | 65.878 (65.46 / 0.19 / 0.24) | 79.800 (78.56 / 1.13 / 0.11) | **61.728** (61.55 / **0.056** / 0.13) | **-22.65%** (undo **20.2x**) |
| Text inserts + final map write | 66.054 (65.60 / 0.20 / 0.25) | 83.914 (78.49 / 5.33 / 0.12) | **62.216** (62.00 / **0.085** / 0.13) | **-25.86%** (undo **62.7x**) |
| Two texts per commit | 504.628 (104.76 / 203.75 / 195.48) | 341.907 (184.43 / 157.28 / 0.30) | **331.864** (173.37 / 158.36 / 0.29) | **-2.94%** (-34.2% vs pre-stack) |
| List push per commit | 117.514 (75.99 / 17.22 / 24.21) | 243.737 (127.73 / 76.10 / 40.41) | **146.639** (**73.71** / 54.34 / **18.82**) | **-39.84%** (edit -42.3%, redo -53.4%) |
| Backward text deletion | 537.057 (484.37 / 52.34 / 0.20) | 558.767 (538.42 / 19.86 / 0.12) | **546.408** (526.83 / 19.70 / 0.12) | **-2.21%** |
| Separated text insertions | 457.136 (74.00 / 228.82 / 154.51) | 317.540 (87.21 / 149.48 / 80.99) | **302.863** (**71.89** / 150.09 / 81.05) | **-4.62%** (-33.7% vs pre-stack) |

### Targeted algorithmic regressions (`v2-extra.rs`, 8 alternating samples per cell)

| Workload | Prior PR #1314 (`77e3fa2f`) | Optimized (`v2`) | Speedup |
| --- | ---: | ---: | ---: |
| 5,000 separate commits under `max_undo_bytes = 4 MiB` | 342.309 | **17.075** | **20.05x** (-95.01%) |
| 50,000 `map.get` hits on a 2,000-entry snapshot-imported map | 781.793 | **4.753** | **164.48x** (-99.39%) |
| 500 `jsonpath` scans over 2,000 snapshot-imported books | 206.510 | **192.429** | **1.07x** (-6.82%) |
| 100,000 scalar `Map::for_each` over 8 keys | 63.657 | **61.521** | **1.03x** (-3.36%) |
