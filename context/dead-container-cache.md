# Dead Container Cache

Verified against code 2026-10-01.

`DocState::is_deleted` (`crates/loro-internal/src/state/dead_containers_cache.rs`)
answers whether a container is unreachable from a root: it walks the arena
parent chain and asks each parent whether it still holds the child
(`DocState::contains_logical_child`). It runs for every local op
(`Transaction::apply_local_op` rejects ops on deleted containers) and for each
handler's `is_deleted()`.

## What is cached

A deleted answer is cached for the container whose edge is broken and every
container below it on the walked chain, in one of two sets:

- `final_deletions`: the parent that lost the chain is a Map (a key only
  changes to a newer value, so a replaced or deleted child never becomes the
  value again) or a List (deletes are final). While the state is at the
  oplog's latest version, no later version gives these back. While it is
  behind (after `checkout`, or detached while imports arrive), a container
  created after the state's version is also cut at its map or list parent,
  and the next checkout forward brings it in.
- `revivable`: the parent is a Tree or a MovableList. Moving a node, or one of
  its ancestors, out of a deleted subtree revives the node's metadata and
  everything below it (a local `mov`, an imported move, or a concurrent move
  whose lamport is greater than the delete's). A movable-list delete
  concurrent with a move of the same element keeps the element when the move
  has the greater lamport, so importing the move revives it.

Not cached: children behind a mergeable edge (the parent map's marker can come
back) and containers without a parent in the arena (a later op can still
attach them).

Release builds return a cached `true` without walking; debug builds always
walk and assert that no cached entry on the chain contradicts the result.

## Invalidation

- `DocState::apply_diff` clears both sets when the direction mode is
  `Checkout` (moving backwards can revive anything) or the diff comes from a
  checkout (`EventTriggerKind::Checkout`). The latter covers forward
  checkouts (`attach`, `checkout_to_latest`, `checkout` to a newer version,
  and `import_batch`'s reattach), whose direction mode is `Import`, `Linear`,
  or `ImportGreaterUpdates`. The only checkout triggered as `Import` is the
  snapshot import into an empty document, whose state is new.
- `DocState::apply_diff` in any other mode clears `revivable` when the batch
  touches a Tree or MovableList container.
- `DocState::apply_local_op` clears `revivable` before a tree `Move`. A local
  op cannot address a deleted movable-list element.
- Resets replace the cache.
- A failed import's rollback clears both sets
  (`DocState::forget_parent_link_caches_after_failed_import`, called by the
  `OpLog` rollback methods): an answer found through a parent link the
  rollback drops is not true of the kept history. See
  [failed-import-arena-indices.md](failed-import-arena-indices.md).

`DeadContainersCache::clear_revivable` replaces the set with a fresh empty set,
releasing its allocation. Its entries are plain `ContainerIdx` values, so this
does not walk the table. The full `clear` also uses this path for `revivable`;
it retains the `final_deletions` allocation. Reusing the revivable table with
`HashSet::clear` would scan its peak capacity whenever a deleted-container
query inserts even one entry before the next move, making a loop of queries
and moves quadratic after caching many deleted containers. Empty caches need
no allocation.

Undo, redo, `revert_to`, and `apply_diff` do not revive container IDs by
themselves: `Handler::apply_diff` creates a new tree node for a deleted target
and remaps its metadata, and map/list children are recreated with new IDs.
Their tree moves still go through `apply_local_op`. Only mergeable children
keep their IDs, and those are not cached.

Before 2026-09-28 every deletion went into one set that forward transitions
never cleared (`clear_alive` removed nothing, since only deletions were
stored), so after `is_deleted()` a revived container still counted as deleted
in release builds and rejected edits until the document was reloaded. The
same held for a container queried at an older version and brought in by a
forward checkout.

The debug assertion runs while the caller holds the state lock, so a stale
entry aborts the process (the unwinding code locks the poisoned state again),
like any other internal assertion under that lock.

## Tests

- `crates/loro/tests/dead_container_cache.rs`: every revival route. Run it with
  `--release` as well; debug builds recompute the answer.
- Unit tests in `dead_containers_cache.rs` check the cache entries directly,
  so they also fail in debug builds when an invalidation is missing.
- `crates/loro/tests/dead_container_cache_perf.rs`: ignored release-mode
  regression that measures three repeats at 32k and 128k deleted nodes, with
  one deletion query per local tree move and a moves-only control. It asserts
  a median `time(4n)/time(n)` ratio below 8, not a wall-clock limit.
