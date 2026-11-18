# Local Commits After a Snapshot: No Lookup in the Encoded Store

Verified against code 2026-11-17.

## What was slow

With a local-update subscriber, every local commit exports the change it has
just made: the commit hook in `crates/loro-internal/src/txn.rs` calls
`export_fast_updates_in_range`, and `ChangeStore::export_blocks_in_range`
(`crates/loro-internal/src/oplog/change_store.rs`) reaches
`load_blocks_in_range` through `stream_span_blocks` and
`overlapping_blocks_for_span`. `load_blocks_in_range` makes sure the blocks of
the span are in `mem_parsed_kv`, the parsed cache.

It asked the encoded store (`external_kv`, a `MemKvStore` or the out-of-core
`FileKvStore`) even when the cache already held the span:

1. `kv.scan(start, end)`: `SsTableIter::new_scan`
   (`crates/kv-store/src/sstable.rs`) reads the SSTable block the start key
   falls in and the block the end key falls in.
2. Unless a block starts exactly at `start`, `ensure_id_lte`:
   `kv.scan(Unbounded, Included(id)).rfind(..)`, which reads the first block
   of the table and the block `id` falls in.

A doc that never exported or imported a snapshot has an empty encoded store,
so this cost nothing. After a snapshot export (history is flushed into the
store) or import (the doc starts from it) the store holds all history, and a
new change's key falls in or next to the last block of some peer.

Reading an SSTable block decompresses it unless the block cache has it. The
default per-document cache is bounded by `BLOCK_CACHE_MAX_BYTES` (4 MiB), so
the lookups are cheap while the blocks they touch fit, and cost a
decompression per commit once they do not: one text insert of 5 MB is one
change block of that size and never fits. Measured per one-character commit,
release build, 200 commits, median:

| History | After a snapshot export or import | Never snapshotted |
|---|---|---|
| One insert of 4 MB | 0.006 ms | 0.006 ms |
| One insert of 5 MB | 5.7 ms | 0.005 ms |
| One insert of 8 MB | 6.8 ms | 0.005 ms |
| One insert of 20 MB | 22 ms | 0.005 ms |
| 8 MB in 4 KB commits, typing peer id above the author's | 1.2 ms | 0.006 ms |
| 20 MB in 4 KB commits, typing peer id above the author's | 5.0 ms | 0.005 ms |

Which peer id types matters only because it decides which block the new key
lands next to. (On 1.17.1 a 1 MB insert already cost about 2 ms per commit;
sizing the block cache to the table, loro-dev/loro#1260, is the likely reason
it no longer does.)

## What it does now

`load_blocks_in_range` returns at once when `ChangeStoreInner::caches_range`
finds a gapless run of cached blocks of the peer over the whole range. That
rests on two facts the store already relies on (`load_parsed_block` trusts a
cached block that holds an id; the scan loop skips keys that are cached):
blocks of one peer never overlap, and a cached block is the current version of
the block with its key. A range that is only partly cached, or unbounded,
takes the old path unchanged. Compaction (`compact_parsed_blocks_in`) evicts
flushed blocks but keeps each peer's last one, so a commit's own change stays
on the fast path.

With it every row of the table above is 0.005 to 0.006 ms. Nothing encoded
changes: the export reads the same blocks for the span as before.

One error path differs. For a fully cached range the old code could still
load the preceding encoded block of the same peer (`ensure_id_lte`), and
`overlapping_blocks_for_span` failed if that block could not be decoded, so
the export stopped at that span. A fully cached range no longer loads or
checks that block.

## Tests

- `caches_range_needs_a_gapless_run_of_cached_blocks_of_one_peer` in
  `change_store.rs`: a cached run, a gap, another peer, each kind of bound,
  and a change that exists only in the cache.
- `crates/loro/tests/perf_commit_after_snapshot.rs`:
  `local_updates_after_snapshot_match_explicit_exports` compares the
  subscriber's bytes with explicit exports for peers that sort before, at and
  after the snapshot's author; the ignored `perf_typing_after_snapshot` prints
  the timings.

## Not covered

Other callers still scan when the cache does not hold the range, for example
the first read of old history after loading a snapshot. That is a one-time
load per block, not a per-commit cost.
