# Arena Parent Links

Verified against code 2026-09-28.

`SharedArena` (`crates/loro-internal/src/arena.rs`) stores each container's
parent. Liveness (`DocState::is_deleted`), paths (`DocState::get_path`,
`LoroDoc::get_path_to_container`), depth (`SharedArena::get_depth`, used by
diff calculation and `ContainerWrapper::new`), and event routing
(`SharedArena::with_ancestors`) all walk it.

## Where a parent link comes from

1. Parsing a change: `register_container_and_parent_link` (`parent.rs`) links
   every container an op creates to the op's container. Local ops do the same
   through `DocState::set_container_parent_by_raw_op`, and a tree `Create` diff
   through `DocState::register_meta_parents_of_created_tree_nodes`.
2. Registering a mergeable root, whose ID encodes its parent.
3. The state KV (`parent_resolver`, installed by `InnerStore::decode` when a
   snapshot is loaded): a container's own state entry records its parent.
4. The op log (`creator_resolver`, installed by `OpLog::new` from
   `ChangeStore::creator_resolver`): a normal container's ID is the ID of the
   op that created it, so loading that op's change block registers the link
   through (1).

`SharedArena::get_parent` and `get_depth` try them in that order.
`DocState::does_container_exist` (behind `get_container`, `has_container`, and
WASM `getContainerById`) calls `SharedArena::find_created_container` for a
normal ID that is not registered yet.

## Why (4) is needed

A document loaded from a snapshot, or created by `fork()`, parses its change
blocks lazily, so (1) has not run for most of the history. A full snapshot has
a state entry only for containers that were materialized. The metadata of a
tree node created inside an already deleted subtree (a concurrent child of a
deleted parent) is never materialized, so neither (1) nor (3) knew its parent,
and `is_deleted()`, edits, and events touching it panicked with "Parent is not
registered" under the state lock (loro-dev/loro#1158). Shallow snapshots are
not affected: shallow export ensures every container of the retention walk,
which includes the metadata of deleted nodes.

## Locking

The creator resolver takes the change store's locks, then parses, which takes
the arena's lock. So:

- the arena calls it without holding its own lock. `ArenaContainers::get_depth`
  runs under the arena lock, so `SharedArena::get_depth` first resolves the
  missing links of the ancestor chain;
- the change store never resolves an arena parent while holding `inner`.
  `ChangeStore::visit_all_changes` runs its callback under that lock; none of
  its callers resolve parents.

It holds weak references to the change store, so the arena does not keep the
store alive. `SharedArena::fork` drops it (it would read the source document's
op log).

## When there is no parent

With the creator resolver installed, `get_parent` returns `None` for a normal
container that no source knows: no op in the history creates it (an ID from
the user that is not a container, or an op not received yet), or it is a
shallow document whose creating op was trimmed and whose state does not hold
the container (shallow export keeps everything that can be alive or revived,
so such a container is dead at the root). No path leads to it, so callers
treat it like a deleted container: `is_deleted` answers `true` (and does not
cache it), `get_path` and `get_depth` answer `None`. An arena without a
creator resolver (not owned by an op log) still panics.

## Import rollback

A failed import can parse old change blocks while it computes its diff
(including through the creator resolver) or validates movable-list ops
(`OpLog::resolve_movable_list_elem`), which registers containers. The arena
rollback (`SharedArena::rollback`) drops registrations made during the import,
so `ChangeStore::rollback_import`, after truncating and evicting blocks (see
[movable-list-op-validation.md](movable-list-op-validation.md)), drops the
parsed changes of every kept block that still has its encoded bytes; the next
access parses again and registers again. Blocks without bytes were built from
changes inserted before the import, so their containers were registered then.
Before 2026-09-28 the parsed ops kept indices that later registrations reuse,
and exporting the history hit `unreachable!` in the JSON encoder.

## Testing pitfall

Under `cfg(test)`, `ChangeStore::import_all` parses the last change of every
peer at load. Unit tests in `loro-internal` therefore do not see lazily parsed
blocks unless the change is not the peer's last; the `loro` crate's tests run
the real path.

## Tests

- `crates/loro/tests/unregistered_container_parent.rs`: repros for every entry
  point with snapshot, fork, and shallow sources, the dumped state from the
  review harness, the import rollback, and a random comparison of every tree
  meta against a full-history import (`UNREGISTERED_PARENT_SEEDS=0..300`).
- Unit tests in `arena.rs`: the resolver runs outside the arena lock, and the
  `None` / panic cases.
