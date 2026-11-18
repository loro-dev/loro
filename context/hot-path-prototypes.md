# Hot-path prototypes

Verified against code 2026-11-18.

These are experimental changes on `2370ab89` plus the earlier candidate patch.
They have not been merged. Benchmark results and validation limits belong to the
separate Tonbo experiment report.

The reproducible native workload and retained measurements are in
`crates/loro/examples/perf_hot_paths.rs` and
`benchmarks/hot-paths-20261118/README.md`.

## Default transaction and read overhead

`LoroDocInner::undo_manager_registered` is a monotonic hint published while
registering the first undo manager. Before registration, local operations avoid
the manager registry lock. Once set, the hint stays set and transactions check
the live weak references, including after a manager is dropped or replaced.

`Transaction::commit` skips a document upgrade for budget admission only when
both its captured document quota and its budget rollback scope are absent.
The decoded-value read path skips cache bookkeeping only if there is neither
an evictable value nor previously accounted cache bytes. Dirty marking and
stale-byte subtraction remain active.

`allocation/dfs.rs` is the independent DAG reference algorithm used in tests.
Explicit work lists avoid recursive stack growth; per-traversal visited sets
avoid repeatedly visiting shared ancestors. Each escape-candidate reachability
query retains its own visited set.

## Transaction mark capacity

`DocState::changed_idx_in_txn` pins locally changed containers during reclamation.
`clear_local_rollback_stack` clears it at each commit. A large initial transaction
can leave a large hash table whose control bytes are reset again for every later
small commit.

`clear_changed_idx_in_txn` discards the allocation if capacity is greater than
four times the current entry count, with a floor of 64. Otherwise it reuses the
allocation. This keeps similarly sized consecutive batches from reallocating,
while a transition from a large batch to a small one releases old capacity.
Membership and the point at which it is cleared remain unchanged.

## Rejected arena read fast path

An experiment made `SharedArena::register_container` look up existing indices
under a read lock before using the original write-locked path on a miss.
Although this skipped unnecessary dirty marking, root-handler lookup slowed
from 16.143 ms to 16.797 ms per 100,000 lookups (about 4%). The experiment was
removed; registration and its lock/dirty-mark behavior remain unchanged.

## Shared handler ownership

`BasicHandler::doc` uses `HandlerDoc`, with direct and shared ownership variants.
Ordinary root and child lookups retain direct ownership. List iteration and
JSONPath map-value iteration create one shared `Arc<LoroDoc>` for the traversal;
its descendants and their clones reuse it. Obtaining an independent document
handle still clones the underlying `LoroDoc`. The last shared owner drops the
document through the existing drop gate, so registry references still cannot
suppress the final commit.

The intended benefit is fewer document drop-gate acquisitions when a query
creates and discards many child handlers. The first prototype allocated a shared
owner for every root lookup and regressed that workload. The revised prototype
limits the allocation to bulk iteration. The ownership enum adds a tag to the
handler, so root and scalar child lookup remain separate benchmark cases.

`BasicHandler::with_doc_state` borrows `self.doc.state` for the lock guard's
lifetime instead of cloning its `Arc` on every read. `&self` keeps the owner
alive; access-tick updates and the scope of the state lock are unchanged.
The matched native experiment and remaining scalar lookup costs are recorded
in `benchmarks/hot-paths-20261118/followup.md`.

## Compact text inverses

`RecordedOp::TextInsert` keeps the event-coordinate position and length without
building a delta. A one-operation commit stores them with its container ID in
`CommitInverse::text_insert`. Its full diff is materialized by
`CommitInverse::materialize_diff` when `LocalInverse::seal` needs it.

Adjacent insertion records for the same container coalesce when the next insert
falls within the previously inserted run, including either boundary. This makes
a long run of typing within one transaction retain one inverse, rather than
composing thousands of deletion deltas at commit. Deletions, other containers,
and insertions outside the run break that coalescing.

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

When every commit of a grouped item inserts into the same continuous text run,
`LocalInverse::coalesced_text_insert` combines the positions and lengths before
`seal` materializes one deletion. This avoids moving repeated delta composition
from editing into the first undo. Insertions outside the run, another container,
or any other operation retain the general path.

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
