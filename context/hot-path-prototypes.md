# Hot-path prototypes

Verified against code 2026-11-18.

These are experimental changes on `2370ab89` plus the earlier candidate patch.
They have not been merged. Benchmark results and validation limits belong to the
separate Tonbo experiment report.

The reproducible native workload and retained measurements are in
`crates/loro/examples/perf_hot_paths.rs` and
`benchmarks/hot-paths-20261118/README.md`.

## Default transaction and read overhead

`LoroDocInner::active_undo_managers` (`lib.rs`, `undo.rs`) is an `AtomicUsize`
incremented when an `UndoManager` registers and decremented in `StatsMarkOnDrop`
when it is dropped. When zero, local transactions avoid allocating or locking
undo-inverse recording state, including after all managers are dropped.

`Transaction` (`txn.rs`) avoids heap-allocating a per-transaction `on_commit`
closure (`emit_on_commit: bool` calls `LoroDoc::emit_post_commit` directly, using
`LoroDoc::borrow_from_inner` so commits never construct/drop a counted `LoroDoc`
handle). `SubscriberSet::is_empty_fast` (`utils/subscription.rs`) exposes a
lock-free `non_empty` atomic flag; when `pre_commit_subs` is empty, `_commit`
skips allocating `ChangeModifier` and collapses three `OpLog` lock acquisitions
into one. When only root subscribers exist (`!has_container_subs`), `Observer`
skips `arena.with_ancestors` per container diff.

`Transaction::commit` skips a document upgrade for budget admission only when
both its captured document quota and its budget rollback scope are absent.
`InnerStore::with_container_for_read` (`inner_store.rs`) preserves
`StorePage::cached_state_bytes` on read hits unless a wrapper transitions into a
materialized `State` (including `RichtextState` `LazyLoad::Src -> Dst`), and
`ContainerWrapper::try_mark_value_cache_hit` (`container_wrapper.rs`) updates the
second-chance bit in place using `LazyDecodedValue::cached_bytes` without
re-walking decoded maps/lists or invalidating page state statistics.

`LoroDoc::resolve_and_ensure_container` (`loro.rs`) registers a container in
`SharedArena` once and constructs the attached handler with `ContainerIdx`
(`Handler::new_attached_with_idx`), avoiding duplicate arena lock acquisitions on
`get_map`/`get_text`/`get_list`/`get_movable_list`/`get_tree`/`get_counter`.
`SharedArena::register_container` marks `stats_dirty` only when a new container
is inserted.

`allocation/dfs.rs` is the independent DAG reference algorithm used in tests.
Explicit work lists avoid recursive stack growth; per-traversal visited sets
avoid repeatedly visiting shared ancestors. Each escape-candidate reachability
query retains its own visited set.

## Transaction mark capacity

`DocState::changed_idx_in_txn` (`state.rs`, `ChangedIdxSet`) stores the first
locally changed `ContainerIdx` inline (`first: Option<ContainerIdx>`) and spills
to `FxHashSet<ContainerIdx>` only when a transaction mutates two or more distinct
containers. Single-container transactions do zero hashing, heap allocation, or
table clearing at commit. When the spillover table grows past four times the
current entry count (floor 64), `clear_changed_idx_in_txn` shrinks it back while
reusing capacity across similarly sized multi-container batches.

## Rejected arena read fast path

An experiment made `SharedArena::register_container` look up existing indices
under a read lock before using the original write-locked path on a miss.
Although this skipped unnecessary dirty marking, root-handler lookup slowed
from 16.143 ms to 16.797 ms per 100,000 lookups (about 4%). The experiment was
removed; registration and its lock/dirty-mark behavior remain unchanged.

## Shared handler ownership and lock-free user handle counting

`LoroDocInner::user_handles` (`lib.rs`) tracks live user-facing `LoroDoc` and
attached handler owners with a lock-free `AtomicUsize`, replacing `DocDropGate`.
`BasicHandler::doc` uses `HandlerDoc`, with direct and shared ownership variants.
Ordinary root and child lookups retain direct ownership. List iteration,
`MapHandler::for_each`, and JSONPath map-value iteration create one shared
`Arc<LoroDoc>` on encountering the first container value; its descendants and
their clones reuse it. Empty and scalar-only traversals create no shared owner.
The optional owner lives outside the state-lock closure so its final drop runs
after the lock is released. Obtaining an independent document handle still clones
the underlying `LoroDoc`. The last user handle commits lock-free while registry
references (`RegistryRef`) never touch `user_handles`.

`BasicHandler::with_doc_state` borrows `self.doc.state` for the lock guard's
lifetime instead of cloning its `Arc` on every read. `&self` keeps the owner
alive; access-tick updates and the scope of the state lock are unchanged.
The matched native experiment and remaining scalar lookup costs are recorded
in `benchmarks/hot-paths-20261118/followup.md`.

## Compact text and list inverses, and O(1) undo stack byte accounting

`RecordedOp::CompactInsert` (`txn.rs`) keeps the event-coordinate position and
length without building a `DeltaRope` B-tree for both `Text` and `List`
insertions. A one-operation commit stores them with its container ID in
`CommitInverse::compact_insert` (`undo.rs`). Its full diff is materialized by
`CommitInverse::materialize_diff` only when `LocalInverse::seal` needs it.

Adjacent insertion records for the same text or list container coalesce when the
next insert falls within the previously inserted run, including either boundary:
both within a single transaction (`LocalInverseRecorder::push`) and in place
across grouped commits (`LocalInverse::chain` via `CommitInverse::try_coalesce_next`).
This makes a long run of typing or list pushes retain one `CommitInverse` rather
than thousands of `Arc<CommitInverse>` entries or `DeltaRope` trees. Deletions,
other containers, and insertions outside the run break that coalescing.

`LocalInverse` caches `commits_bytes` incrementally as commits are chained, and
`undo::Stack` maintains `cached_bytes: Option<usize>` incrementally across push,
merge (`try_merge_top_from_events` / `push_with_merge_cids`), `pop_front_inner`,
and `clear` (which is a zero-allocation no-op when already empty). Enforcing
`max_undo_bytes` on every commit is therefore $O(1)$ amortized rather than
re-walking the retained stack in $O(N^2)$.

Other multi-operation transactions materialize the per-operation delta during
commit and use the existing reverse composition. Mergeable containers
retain the existing fallback. Newly created containers are filtered as before;
when commits are grouped, the combined span's final pruning still runs.
Counter summation and movable-list records are unchanged.

The compact field and retained container ID contribute to the inverse memory
estimate. Commit and local-inverse allocation headers, struct storage and spare
record-vector capacity are included too. A single `LocalInverse` commit reference
is kept inline with `SmallVec`; longer groups spill into a vector and retain
shared commit references.

Tests include concurrent final child-handle drops, grouped Unicode inserts into
a newly created text container, and the large-to-small transaction capacity
transition. Debug tests should run with `LORO_VERIFY_UNDO_LOG=1`.

When every commit of a grouped item inserts into the same continuous text or list
run, `LocalInverse::chain` coalesces them in place (and
`LocalInverse::coalesced_compact_insert` fast-paths `seal` into a single deletion).
This avoids moving repeated delta composition from editing into the first undo.
Insertions outside the run, another container, or any other operation retain the
general path.

The general path also needs bounded delta composition. Previously, a deletion's
end lookup skipped zero-output deletion leaves at that boundary, and forward
fixed-offset deletions left adjacent pure-deletion leaves unmerged across leaf
boundaries. `DeltaRope::_compose_replace` now stops at the last consumed output
unit (`range.end - 1` + 1), merges multi-leaf pure deletions into a single
`Delete` leaf (or into an immediately preceding `Replace` leaf) via
`_delete_batch_leaves`, and merges single-leaf boundary deletions into their left
neighbour. See
[text delta composition](text-delta-chunking.md#deletion-composition-boundaries).
This does not change commit ordering, map/counter handling, or fallback rules.
It removes this quadratic suffix scan, not every possible quadratic delta
workload.

Grouped sealing also borrows each existing commit diff instead of cloning a
temporary `DiffBatch` for every commit. Compact text records delete directly from
the accumulated text delta after its first entry is materialized. Map writes
still compose in reverse commit order with the later inverse taking precedence;
counter recomputation, final pruning and normalization remain unchanged.

Mixed text inverses that both restore text and delete text in the same container
fall back to checkout, both for a single transaction and after grouped commits
are composed. Seed 3 of `undo_compact_text` exposed different insertion positions
within a deleted range; matching final text alone is insufficient to establish
equivalent transformation against later remote edits. Pure insertion and pure
deletion inverses remain recorded. This fallback can allocate history caches
for mixed replacements.

The debug inverse comparator expands `Replace` in the iterator's execution
order: insertion before deletion. Its tests compare a replacement with a split
deletion, including transformations against remote insertions at each boundary,
and reject an insertion moved across deleted input.


## CRDT benchmark workload follow-up

The subscription workloads exercise local update encoding, JS event delivery
and short string conversion together. `PendingEvent::check_scheduled` caps
checks per JS turn, even if each of many synchronous commits drains the queue.
Small local updates encode from the change-store insertion path and use the
stored slice when commits merge. Rich text caches five position indexes inline
and keeps an eligible text leaf across replacement edits. See
[internal encoding](internal-encoding.md#local-updates-and-small-blocks),
[rich-text insert positions](richtext-insert-positions.md#the-insert-cursor-cache),
and the [WASM event invariant](../crates/loro-wasm/AGENTS.md#pending-events-invariant).
The [CRDT benchmark report](../benchmarks/crdt-hotpaths-20261118/README.md)
records the release WASM A/B measurements.


The follow-up's `CrdtRope::first_non_future_at_or_after` counts non-future
atoms in each B-tree subtree (`Cache::non_future: u32`, using mod-$2^{32}$
wrapping diffs so `Cache` stays 12 bytes). Deleted atoms still count toward the
right origin, and splitting a run keeps the sum unchanged. It probes `start`
and up to four adjacent leaves via `BTree::next_elem` in $O(1)$ first, falling
back to the B-tree rank query only on longer future runs. The second scan
follows the same origin and peer ordering rules with a streaming iterator and
records visited spans only when a different left origin needs a membership
check. Debug builds compare the first search with a linear scan.

To bound peak WASM linear memory during large concurrent text/list merges:

- `FugueSpan` (`fugue_span.rs`) is 64 bytes (compile-time asserted): `diff_status`
  uses a 4-byte `OptionalStatus`, and `UNKNOWN_PEER_ID` spans store their
  revealed `(base_real_id, base_unknown_counter)` out of line in
  `CrdtRope::unknown_real_ids`, indexed by `FugueSpan::id.lamport`
  (`UNKNOWN_REAL_ID_UNSET = u32::MAX` until deleted; `FugueSpan::_slice`
  preserves `id.lamport` on `UNKNOWN_PEER_ID` spans).
- One-shot `DiffCalculator::new(false)` (`diff_calc.rs`, `loro.rs`, `state.rs`)
  releases each container calculator's transient tracker/style/text/value
  allocations immediately after `calculate_diff` (`release_transient_memory`) while
  retaining the `ContainerIdx` keys for `materialize_touched_roots`, and drops
  old trackers before `build_full_tracker` / `build_full_crdt_tracker` on
  `should_rebuild`.
