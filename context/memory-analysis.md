# LoroDoc Memory Analysis

Verified against code 2026-11-06 (main `ea1d383e`, after #1218, #1222, #1244).

This is a measurement report for a process that keeps many `LoroDoc`s open for a
long time: a local sync hub holding tens to thousands of documents, with single
documents up to 28 MB. Nothing here changes behavior. Prototypes live in
[`benchmarks/memory-2026-11/prototype.patch`](benchmarks/memory-2026-11/prototype.patch).

## Summary

- **#1218, #1222 and #1244 did not move the stale-write peaks.** Only zero-copy
  import helps, and it removes one copy of the snapshot.
- **Cold documents are already cheap.** With `from_snapshot_bytes` over a file
  mapping, a 28 MB document costs 1.1 MB of heap. Reads, appends and subscribing
  add 0.5–2.2 MB.
- **The cost comes from a few operations that expand the whole history, and
  nothing gives that memory back.** One small `import` adds 28.8 MB. One
  `checkout` adds 56–111 MB. `free_diff_calculator`, `free_history_cache` and
  `compact_change_store` free about nothing after a near-head checkout.
- **The shadow document costs double because it repeats that expansion.** Each
  document has its own parsed blocks, DAG and arena.
- **Prototypes cut most of it without a format change.** A cold import drops from
  56 ms and +28.8 MB to about 1 ms and +0.01 MB, and that part passes the test suite.
  A detached stale-write loop drops from 91.8 MB to 46.5 MB of heap, and a shadow
  from 159 MB to 62 MB, but those two runs include the near-head vv path, which
  still fails one test target. See [Prototype validation](#prototype-validation).
- **Two defects found:** `ImVersionVector::extend_to_include_vv` destroys
  structural sharing, and shallow snapshots barely trim documents where peers
  alternate, which is the chat shape.

## Method

| Item | Value |
| --- | --- |
| Engine | main `ea1d383e`, crates.io `loro` 1.17.1 as baseline |
| Machine | 4 cores, 7.7 GB RAM, shared with other jobs; runs serialized by a lock |
| Build | release, thin LTO, `debug = 1`, glibc malloc unless noted |
| Heap | a counting `GlobalAlloc` for live bytes, per-stage peak and block counts |
| Attribution | `dhat` (live-at-end and at-peak) with stacks classified by `harness/classify.py` |
| RSS | `RssAnon`, `RssFile`, `VmHWM` from `/proc/self/status` |
| Repeats | Each number is one run unless stated. Treat latency as indicative |

MiB and MB are both 2^20 bytes in the tables. **Measured** means a run in
[`logs/`](benchmarks/memory-2026-11/logs). **Inferred** means derived from measured
parts or from reading code, and it is always labelled.

| Document | Shape | Snapshot | State only | Ops | Changes |
| --- | --- | --- | --- | --- | --- |
| `d28m` | 10,000 turns, list of maps with text, 2 alternating peers | 27.73 MiB | 13.80 MiB | 16.6 M | 10,000 |
| `dlong` | 200 turns, 1 M random single-char edits, 3 editing peers | 8.53 MiB | 2.90 MiB | 1.04 M | 1,157 |
| `d1m` | 1,000 turns | 1.04 MiB | 0.53 MiB | 0.59 M | 1,000 |

`d28m` and `dlong` are the snapshots from the earlier stale-write spike, so the
numbers compare directly. `dlong` is a deliberate worst case: every block touches
most texts.

## Baseline: 1.17.1 vs main

`d28m`, one process per mechanism, 40 writes at d=1 then 40 at d=100 (15 each for
`fork_at`). Measured.

| Mechanism | Version | Live heap | Peak RSS | Write p50 at d=1 |
| --- | --- | --- | --- | --- |
| detached | 1.17.1 | 91.8 MB | 102.2 MB | 30.7 ms |
| detached | main | 91.8 MB | 102.3 MB | 24.3 ms |
| shadow | 1.17.1 | 186.6 MB | 195.5 MB | 0.48 ms, first 256 ms |
| shadow | main | 186.8 MB | 195.3 MB | 0.26 ms, first 246 ms |
| shadow, shared `Bytes` | main | 159.0 MB | 167.6 MB | 0.27 ms, first 279 ms |
| `fork_at` per write | 1.17.1 | 98.2 MB | 217.7 MB | 462 ms |
| `fork_at` per write | main | 98.3 MB | 217.4 MB | 469 ms |

The earlier 129 MB and 220 MB figures came from a loop that went up to d=1000 and
also read the snapshot differently, so absolute values differ. `from_snapshot`
still copies, so its peak is twice the snapshot (56.7 MB). `from_snapshot_bytes`
peaks at 29.0 MB.

## 1. What one LoroDoc holds

Live heap by component, from `dhat`. All on main.

| Component | `d28m` cold | warm | after 1 checkout | after shadow loop (2 docs) | `dlong` after 1 checkout | `dlong` wide checkout |
| --- | --- | --- | --- | --- | --- | --- |
| Snapshot bytes | 27.74 | 27.74 | 27.74 | 27.75 | 8.54 | 8.54 |
| Parsed change blocks | 0 | 0 | 25.21 | 50.43 | 100.24 | 100.24 |
| Decoded KV bytes | 0.02 | 0.32 | 19.22 | 38.43 | 7.95 | 14.85 |
| DAG nodes and vvs | 0 | 0 | 9.89 | 19.87 | 1.42 | 1.42 |
| Arena | 0 | 0 | 2.31 | 4.63 | 0.04 | — |
| State | 0.57 | 2.63 | 2.59 | 9.97 | 0.24 | 62.9 |
| SSTable metadata | 0.50 | 0.50 | 0.50 | 0.99 | 0.07 | 0.07 |
| History cache | 0 | 0 | 0 | 3.17 | 0 | 0 |
| Diff calculator | 0 | 0 | 0.00 | 0.00 | 0.10 | 206.90 |
| **Total (MiB)** | **28.83** | **31.20** | **87.47** | **155.32** | **118.58** | **395.07** |

"Warm" means a read of 20 turns and one append. "Wide checkout" is 40,000 ops back,
across all 200 texts.

| Component | Grows with | Measured rate |
| --- | --- | --- |
| Parsed blocks | ops parsed, not released | 90 B/op with `Vec` slack; `size_of::<Op>()` is 64 |
| Decoded KV bytes | history size, not released | 17 MiB for `d28m`, about the uncompressed oplog |
| DAG | changes | 990 B/node, 755 B of it the per-node `ImVersionVector` |
| Arena | containers | 107 B/container |
| List state | list length | 200 B/entry |
| Tracker | ops in the containers touched | 1 KB per text in `dlong` near head, 1 MB per text when wide |
| Block cache | reads | Bounded, 4 MiB per table, 2 tables per doc |

Two details matter:

- **The "4 MiB" block cache bound does not bound history bytes.** The 17 MiB is
  `NormalBlock::decode` output. `ChangesBlock::from_bytes` keeps a `Bytes` clone in
  `mem_parsed_kv`, so it outlives cache eviction. The history then exists three
  times: compressed in the snapshot, decoded in KV bytes, parsed as `Op`s.
- **The diff calculator is usually not the problem.** Near head it retains about
  nothing. It only matters for wide checkouts on long histories, where
  `free_diff_calculator` releases 207 MiB.

## 2. Lazy loading after a cold open

Each row is a fresh process: `from_snapshot_bytes`, then the operation. Values are
added to the 28.83 MB and 8.77 MB cold totals. Measured on main.

| Operation | `d28m` retained | `d28m` peak | ms | `dlong` retained | `dlong` peak | ms |
| --- | --- | --- | --- | --- | --- | --- |
| List length | +0.53 | +0.65 | 1 | +0.02 | +0.13 | 0 |
| Read 20 turns | +0.73 | +0.85 | 1 | +0.44 | +0.56 | 1 |
| Cursor resolve | +0.54 | +0.66 | 1 | +0.27 | +0.51 | 1 |
| Append text | +2.18 | +2.71 | 10 | +0.43 | +0.51 | 1 |
| Push a turn | +2.18 | +2.70 | 5 | +0.09 | +0.22 | 0 |
| Subscribe, then append | +2.18 | +2.71 | 6 | +0.43 | +0.51 | 1 |
| `UndoManager`, then append | +2.18 | +2.71 | 5 | +0.43 | +0.51 | 1 |
| Export snapshot | +0.62 | +33.39 | 168 | +0.01 | +12.02 | 21 |
| `fork()` | +1.72 | +33.39 | 160 | +0.25 | +11.85 | 15 |
| Read all texts | +13.38 | +13.97 | 80 | +4.10 | +4.22 | 4 |
| `get_deep_value` + JSON | +6.31 | +45.57 | 163 | +3.31 | +4.40 | 6 |
| **Import one small update** | **+28.81** | +29.78 | 56 | **+9.30** | +9.41 | 13 |
| **`frontiers_to_vv` at head** | **+28.81** | +28.93 | 87 | **+9.11** | +9.12 | 12 |
| **Export all updates** | **+47.30** | +99.98 | 186 | **+108.01** | +202.72 | 390 |
| **`checkout` one step back** | **+56.47** | +59.60 | 175 | **+110.74** | +110.97 | 204 |
| **`diff` one step back** | **+58.63** | +59.59 | 180 | **+109.86** | +110.99 | 205 |
| **`fork_at` head** | **+89.83** | +128.47 | 658 | **+121.42** | +204.12 | 423 |

Subscribing costs nothing by itself. The first event builds the root list state
through `get_path`, which is the +2.18 MB in the subscribed import.

Three paths force the expansion. All three were found from `dhat` stacks:

1. **Version vectors walk to the root.** `AppDag::try_ensure_vv_for` is a DFS with no
   shortcut, so asking for the vv of the head with nothing cached loads every DAG
   node, and through `ensure_block_loaded_in_range` every block. A one-op import hits
   this at `try_iter_from_replay_base_causally_filtered`, which recomputes a vv the
   caller already passed in as `from`.
2. **A rollback rebuilds the text tracker from nothing.** Checking out backwards sets
   `should_rebuild`, then `replay_container_ops_from_empty` iterates the whole
   history. `uncovered_entry_parents` also parses every change, although nothing can
   be uncovered when `from` is empty. The container filter runs on parsed changes,
   so every block is parsed to find a few ops.
3. **Anything that parses a block keeps it.** `ensure_changes` turns a block into
   `Both` in `mem_parsed_kv`, and nothing evicts it.

### Release paths

| API | After near-head checkout | After wide checkout | After export of all updates |
| --- | --- | --- | --- |
| `free_diff_calculator` | 0.00 | −206.9 MB on `dlong`, −7.1 MB on `d28m` | 0 |
| `free_history_cache` | 0 | −3.2 MB on `d28m` | 0 |
| `compact_change_store` | 0 | 0 | 0 |
| `malloc_trim(0)` | RSS only | RSS only | RSS −37 MB on `d28m`, −71 MB on `dlong` |
| Drop and reopen | everything | everything | everything |

The doc comment of `compact_change_store` says "The parsed ops will be dropped".
That holds only for blocks that were not flushed yet: `flush_and_compact` skips
flushed blocks, and every block loaded from a snapshot is flushed. On live writes it
released 6.8 MB of 27.9 MB.

## 3. Many documents

Heap per document, linear from 100 to 5,000 documents. Measured on main.

| Document | `from_snapshot` | `from_snapshot_bytes` | RSS anon |
| --- | --- | --- | --- |
| `LoroDoc::new()` | 7.1 KB | — | 7.5 KB |
| plus subscription | 8.2 KB | — | 8.7 KB |
| 428-byte snapshot | 19.7 KB | 19.2 KB | 20.5 KB |
| 1.4 KB snapshot | 27.1 KB | 25.7 KB | 27.1 KB |
| 11 KB snapshot | 45.8 KB | 34.7 KB | 36.8 KB |
| 110 KB snapshot | 163.9 KB | 53.7 KB | 61.0 KB |
| 1.04 MiB snapshot | — | 69 KB | 83 KB |
| 27.7 MiB snapshot | — | 1.15 MB | 1.57 MB |

5,000 documents of 1.4 KB take 122.6 MB of heap and 129.6 MB RSS. 1.17.1 is the
same within 0.1%. The fixed cost has no superlinear term.

In the 1.4 KB case, `dhat` puts 52% in the block caches: 2 `quick_cache` instances
sized for 1,024 blocks each plus the decoded blocks they hold. Each open also churns
433 KB of transient allocations, mostly 64 KB LZ4 buffers. That costs time, not
retained memory.

For large cold documents the cost is per block and per container: SSTable key
metadata (0.5 MB) and the state store (0.57 MB).

### What can be shared

| Candidate | Verdict |
| --- | --- |
| Arena | No. `ContainerIdx` is document-local and baked into every parsed `Op`, state and handler. Arena sharing means one index space per process |
| Parsed blocks across documents | No, for the same reason |
| Strings | Already shared. `InternalString` is inline up to 7 bytes, otherwise in a global reference-counted set. Read from code: no leak |
| Snapshot `Bytes` | Yes, between copies of the same document. That is what a shadow can do |
| Block cache | Yes, and it is worth doing. See item 8 |
| Allocator | No gain. See below |

Allocators, `d28m`, peak RSS in MB. Measured.

| Scenario | glibc | jemalloc | mimalloc |
| --- | --- | --- | --- |
| Detached loop | 100.7 | 114.8 | 105.2 |
| Shadow loop | 164.9 | 172.5 | 172.9 |
| Deep value + exports | 147.3 | 164.1 | 162.0 |
| 2,000 small docs, RSS per doc | 27.1 KB | 28.2 KB | 28.0 KB |

Live heap is identical. glibc with `malloc_trim` is the best of the three here.

## 4. Why a shadow doubles

The shadow does not copy the head. It repeats the expansion, because `checkout`
expands whatever document runs it, and then the import back expands the head. From
the table in section 1: 2 × (25.2 parsed + 19.2 decoded + 9.9 DAG + 2.3 arena + 2.0
list state) + 3.2 history cache, on top of one shared snapshot.

### Options

| Option | Saves on `d28m` | Status |
| --- | --- | --- |
| Share the snapshot `Bytes` between head and shadow | 27.7 MB | **Works today**: `from_snapshot_bytes(bytes.clone())` |
| Fix the expansion (items 1–4 below) | Shadow 159 → 62 MB | Prototyped |
| Share one op log between two `DocState`s | Inferred: the rest of one history copy | Not prototyped |
| A write-at-version API over a sparse state | Inferred: a shadow's whole cost | Not prototyped |

Sharing parsed data between two separate documents is blocked by the arena rule
above. Sharing the op log avoids that, since both states then use one arena.

`LoroDocInner` already holds `oplog` and `state` as separate `Arc`s, and the `OpLog`
doc comment says it "allows multiple [AppState] to attach". Two invariants stand in
the way:

- An attached doc expects `state.frontiers == oplog.frontiers`, so a commit from the
  second state leaves the first one behind until it applies a diff.
- Every lock is created in one `LoroLockGroup` with a fixed order.

Detached mode on a single document is the closest measured stand-in for this design:
91.8 MB today, 46.5 MB with the prototypes. A real implementation adds the base state
of the containers it touches.

The cheapest API would not keep a second state at all:
`doc.edit_at(frontiers, |view| …)` materializes only the touched containers at the
base version, creates ops with `deps = base`, and imports them through the normal
concurrent path. Its memory would be proportional to the touched containers. It
needs a sparse overlay on `ContainerStore`, and it must handle reads of a container
the batch did not expect to touch. This is an inference from the code.

## 5. Optimization list, by benefit

"Saving" is for the hub workload. Scope counts prototype lines where one exists. No
item changes the encoding format.

| # | Change | Saving | Scope | Risk | Evidence |
| --- | --- | --- | --- | --- | --- |
| 0 | **Hub, no engine change:** open with `from_snapshot_bytes` over a mapping, drop idle docs, call `free_diff_calculator` after wide checkouts, `malloc_trim` after exports | 27.7 MB of anon heap per 28 MB doc; idle doc costs 0 heap | Application | Low; a mapped file must not change underneath | Measured |
| 1a | **Reuse `from` as the replay base vv and seed the next node's vv from it** | Cold sequential import: 56 ms, +28.8 MB → 1.3 ms, +0.01 MB; `dlong` +9.3 → +0.2 MB. Applies to every document that receives one update | `loro_dag.rs`, `oplog.rs`, ~30 lines | Low to medium | Prototyped, tests pass |
| 1b | **Resolve vvs near the head without walking to the root** | Also covers concurrent import and `frontiers_to_vv`: `d28m` 56 ms, +28.8 MB → 0.2 ms, +0.01 MB. `dlong` concurrent import still adds 8 MB | `loro_dag.rs`, ~70 lines | High as prototyped: 1 test target fails, and the many-peer regression below | Prototyped, **tests fail** |
| 2 | **`extend_to_include_vv`: read before writing** | 46.3 → 11.6 MB at 802 peers (−75%), p50 10.7 → 7.6 ms | `version.rs`, ~10 lines | Low | Prototyped, A/B |
| 3 | **Bound `mem_parsed_kv`** and drop flushed blocks | With 4: `d28m` detached 91.8 → 46.5 MB, RSS 102 → 57 MB. `dlong` 124.6 → 21.6 MB, RSS 104 → 52 MB. Export leaves 35.5 instead of 76 MB, and 12.9 instead of 118 MB on `dlong` | `change_store.rs`, ~40 lines | Medium: see hazards | Prototyped |
| 4 | **Skip blocks by container before parsing**, and return early from `uncovered_entry_parents` on empty `from` | `d28m` checkout +58.6 → +36.5 MB, stale write p50 28 → 8 ms. No gain on `dlong`, where blocks touch every text | `change_store.rs`, `oplog.rs`, ~60 lines | Low to medium: unparsed blocks are not validated | Prototyped |
| 5 | **Fix the single-head critical walk** (defect below) | Shallow `d28m` 27.71 → 13.91 MiB, stale p50 49 → 0.45 ms, loop heap 110 → 43 MB | `dag.rs`, size unknown | High: needs a sound rule, see below | Measured with an unsound switch |
| 6 | **Lazy decode for shallow and state-only snapshots** | Inferred: a 13.8 MiB state-only doc opens at 15 MB instead of 39.8 MB | `inner_store.rs`, `container_store.rs` | Medium | Measured cost, fix not prototyped |
| 7 | **Size block caches to the table** | 25.7 → 20.1 KB/doc (−22%); 5,000 docs 122.6 → 95.8 MB | `sstable.rs`, ~5 lines | Low | Prototyped |
| 8 | **One process-wide block cache** instead of 2 per document | Inferred: removes up to 8 MiB per touched doc. `get_deep_value` leaves 4.4 MiB today. At 1,000 touched docs that is 4.4–8 GB against one fixed budget | `kv-store`, new shared cache | Medium: key and eviction design, lock contention | Cost measured, fix not prototyped |
| 9 | **Shrink parsed op vectors** after decode | `dlong` parsed blocks 100.2 → 67.2 MiB (−33%) | `block_encode.rs`, 3 lines | Low; CPU cost not isolated | Prototyped |
| 10 | **Second state over one op log, or `edit_at`** | Inferred: a shadow costs about what detached costs, or less | `lib.rs`, `state`, `txn`, large | High: state/op log invariant, lock order, public API | Not prototyped |
| 11 | **Keep parsed ops columnar** | Inferred: 64 B/op toward the 7–9 B/op on disk, `dlong` mainly | `change_store`, large | High: every reader of `Op` | Not prototyped |
| 12 | **Flat `ImVersionVector` for few peers** | Inferred: 7.5 MiB → under 1 MiB on `d28m` | `version.rs`, small | Low to medium: needs the `im` path above a threshold | Stack measured |

Suggested order: 2, 7, 9 and 1a are small and independent. Then 4 with 3. Items 1b
and 5 need design work first.

### Hazards found by the prototypes

- **Eviction can panic.** `insert_change` merges into the previous block in the map.
  With a peer's last block evicted but an older one present, it panicked with
  "counter should be continuous". The prototype keeps the last block of each peer.
- **Per-write eviction is expensive on spread-out histories.** `dlong` d=1 p50 goes
  from 15 ms to 153 ms at a 256-block cap, and heap from 124.6 to 21.6 MB. Eviction
  should be size-based or idle-time, and the cap should be configurable.
- **Peaks barely move for bulk paths.** Item 3 lowers what stays, not what an export
  allocates: `d28m` still peaks at 128 MB, `dlong` at 178 MB. Lowering those needs
  streaming in the export path.
- **The near-head path is not correct yet.** Without a guard it panics on shallow
  documents, because `try_find_path` walks below the shallow root. With the guard,
  `unparsable_history` still fails. The likely cause is that the path skips the walk
  that reports a corrupt block. This was not confirmed.
- **The near-head path regresses the many-peer case.** At 802 peers it leaves 29.0 MB
  where the vv fix alone leaves 11.6 MB. `ImVersionVector::from_vv` builds a full map
  for the hint with no sharing. A dep check did not remove this.
- **Blocks skipped by header are not validated.** A corrupt body in a skipped block
  no longer fails that checkout. The value is still right, but
  `check_history_parsable` sees less.

## Prototype validation

`cargo test -p loro --tests --no-fail-fast` on the prototype, debug build. Full results
are in [`logs/tests.md`](benchmarks/memory-2026-11/logs/tests.md).

| Configuration | Result |
| --- | --- |
| Near-head path off | 76 binaries, 834 tests pass |
| Near-head path off, 8-block cap | 76 binaries, 834 tests pass |
| Near-head path on, no shallow guard | 5 targets fail |
| Near-head path on, shallow guard | 1 target fails: `unparsable_history` |

`loro-internal --lib` passed 477 tests at the 8-block cap, on the build without the
shallow guard. The shallow guard was added after the measurements, and it only affects
shallow documents.

The near-head path also matched the slow path in two stale-write loops
(`LORO_PROTO_CHECK_VV`). That check covers those loops only.

The detached and shadow loop numbers in item 3 ran with the near-head path on. They
were not repeated with it off, so their share from item 1b is not isolated.

## Defects

No unbounded leak was found. Growth comes from retention that has no release path.

### `ImVersionVector::extend_to_include_vv` destroys structural sharing

`crates/loro-internal/src/version.rs`. It calls `im::HashMap::get_mut` for every
peer, and `get_mut` copies the path to the entry even when the counter does not
change. So merging two vvs at a merge node unshares the whole map.

Fresh-peer stale writes, `d1m`, 800 writes, 802 peers, with the near-head path off:

| Variant | Live heap | Peak RSS | p50 |
| --- | --- | --- | --- |
| `get_mut` (today) | 46.33 MB | 54.9 MB | 10.7 ms |
| `get`, insert only on change | 11.63 MB | 19.8 MB | 7.6 ms |

### Shallow snapshots barely trim alternating-peer documents

Reproduced on 1.17.1 and main. A document where two peers take turns has a linear
history, so every version is critical. Asking for a shallow snapshot at a frontier
outside the head node returns the end of the first change instead:

```text
2 peers, 200 turns, head 10199@2
asked 10149@2 -> since 10149@2, 10,225 bytes, 1 change
asked  9699@2 -> since   101@1, 19,585 bytes, 200 changes
```

With one peer the same requests are honored. On `d28m` a shallow export is 27.71 MiB
for any frontier, because it holds the root state, the latest state and nearly all
the history.

`latest_single_head_critical_version` (`dag.rs`) stops when its pending heap narrows
to one span. `deps_to_ord_id_spans` adds the implicit same-peer predecessor unless a
*direct* dep span contains it. In an alternating history the predecessor is only a
transitive ancestor, so a stale entry stays in the heap and the heap never narrows
before the root.

A measurement-only switch that assumes axiom A6 (explicit deps cover the author's
prefix) gives the expected result: 13.91 MiB at 20,000 ops back and 17.91 MiB at
2,000,000 ops back. That switch is **not a fix**. The spec keeps the predecessor
because imported data is not checked against A6
([docs](../crates/loro-internal/docs/critical-version-spec.md)). A real fix has to
prove the predecessor is an ancestor of another pending span.

The same walk serves as the replay fallback in
`try_iter_from_replay_base_causally_filtered`, so replay falls back to ∅ more often
than the DAG requires. How much that costs was not measured.

### Smaller findings

- A wide checkout is slow: 1,000,000 ops back on `d28m` took 37.9 s on main and 14.4 s
  with the prototype, one run each. 6,000,000 ops back did not finish in 200 s. Each
  touched text replays the whole DAG.
- Shallow export 2,000,000 ops back took 67 s with the A6 switch, against 1.1 s at
  20,000 ops back. Not investigated.
- Shallow and state-only open eagerly: `decode_twice` scans all entries, and each
  wrapper pins decoded bytes. A 13.80 MiB state-only snapshot sits at 39.8 MB after
  open, more than the full 27.73 MiB snapshot at 28.83 MB.

## Not verified

- **Real documents.** All numbers come from synthetic documents shaped like chat
  history. Real data will differ in text size and peer patterns.
- **Latency precision.** Most latencies are one run on a shared machine.
- **`dlong` d=1 vs d=100.** At a 256-block cap, d=1 p50 is 153 ms and d=100 is 1.6 ms.
  This was not explained.
- **Prototype scope.** The patch is for measurement and is not ready to merge. Eviction
  under concurrent readers and import rollback has no dedicated test.
- **Fuzzing.** No fuzz target ran against the prototype.
- **Mapped files under memory pressure.** The 32.9 MB `RssFile` is reclaimable in
  principle. Reclaim behavior was not tested, and import checksums touch every page.
- **WASM and macOS.** Not measured.
- **Items marked inferred.** Items 6, 8, 10, 11 and 12 have measured costs but no
  prototype of the saving.

## Reproducing

[`benchmarks/memory-2026-11/`](benchmarks/memory-2026-11/) holds the harness, the logs
and the patch. Paths in the scripts are from the original machine. Wrap builds and
runs in the machine's shared lock. Check binary timestamps before reading a log: a
failed build leaves the previous binary in place, and one run in this work measured a
stale binary until `build_all.sh` was changed to fail hard.
