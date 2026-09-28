# Arena Parent Links

Verified against code 2026-09-29.

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

The creator resolver is the only access to the change store that does not hold
the document's op log lock. `is_deleted`, `has_container`,
`get_path_to_container`, and `ContainerWrapper::new` reach it under the state
lock, and event emission (`with_ancestors`) under no document lock. So:

- `ChangeStore` takes its locks in one order, documented on the struct:
  `root_history_names`, `external_kv`, `inner`, `external_vv`, and the arena's
  lock last (parsing registers containers). Before loro-dev/loro#1159's review,
  `get_parsed_block_in`, `flush_and_compact`, and `get_change_by_lamport_lte`
  took `inner` before `external_kv`, which was harmless while the op log lock
  serialized every caller. With the resolver, `is_deleted` on one thread and
  `len_changes`, `export`, or `vv_to_frontiers` on another deadlocked.
  `ChangeStore::load_parsed_block` looks for a cached block under `inner`
  alone, then takes `external_kv` and `inner` in order and looks again before
  it loads the block from the KV store.
- The arena calls the resolver without holding its own lock.
  `ArenaContainers::get_depth` runs under the arena lock, so
  `SharedArena::get_depth` first resolves the missing links of the ancestor
  chain. It fails fast on a cycle, which would otherwise loop.
- The change store never resolves an arena parent while holding `inner`.
  `ChangeStore::visit_all_changes` runs its callback under that lock; none of
  its callers resolve parents.
- Import rollbacks run under the state lock and roll the arena back under
  `inner`; see "Import rollback" below.
- Another thread can register a container at any time without its parent, for
  example by creating a handler. Code that looks a container up twice must not
  assume the answer stayed the same: `DocState::does_container_exist` decides
  from its first lookup.

It holds weak references to the change store, so the arena does not keep the
store alive. `SharedArena::fork` drops it (it would read the source document's
op log).

## When there is no parent

The resolver answers `CreatorOp::Loaded` when a change block holds the op (it
is parsed now, so the parent of every container the op creates is registered)
and `CreatorOp::Absent` when none does. `get_parent` returns `None` for a
normal container that no source knows:

- no op in the history creates it: an ID from the user that is not a
  container, or an op not received yet;
- a shallow document whose creating op was trimmed and whose state does not
  hold the container. Shallow export keeps everything that can be alive or
  revived, so such a container is dead at the root.

No path leads to it, so callers treat it like a deleted container:
`is_deleted` answers `true` (and does not cache it), and `get_path` and
`get_depth` answer `None`. Broken invariants still fail fast:

- The resolver panics when a block holds the op but cannot be decoded or
  parsed, instead of answering `Absent`.
- An arena without a creator resolver (not owned by an op log) panics.
- `ContainerWrapper::new` panics for a container without a parent, so a
  container never gets state without one.
- A local op's containers are linked by the transaction
  (`set_container_parent_by_raw_op`) before the change reaches the op log, so
  the resolver cannot supply a link that a local op path forgot. Debug builds
  check the links when a local change is committed
  (`parent::assert_local_parent_links_registered`).

## Cost of looking up an ID

`has_container` / `get_container` / `getContainerById` for a normal ID that is
not registered ask the resolver:

- No block holds the op (beyond the history, an unknown peer, before a shallow
  root): a KV scan and one block header, with no parse. About 0.5 µs per ID.
- A block holds the op: the block is parsed once and stays cached, like any
  other lazy access to that part of the history. This happens even when the op
  creates no container, because only the op's content tells. The version vector
  or DAG can tell only whether the op is in the history, not what it creates.

Measured with 2k IDs on a 20k-node, 100k-op document, `main` vs the resolver:

| IDs | `main` | resolver |
|---|---|---|
| in the history, first pass | 1.3 ms | 6.1 ms |
| in the history, second pass | 0.66 ms | 0.78 ms |
| beyond the history | 1.1 ms | 2.1 ms |

A vv shortcut for IDs beyond the history would have to be exact for every path
that writes the KV store, or a live container would read as deleted. Measured
2026-09-28 (loro-dev/loro#1159).

## Import rollback

A failed import can parse old change blocks while it computes its diff
(including through the creator resolver) or validates movable-list ops
(`OpLog::resolve_movable_list_elem`), which registers containers and allocates
their values. The arena rollback (`SharedArena::rollback`) drops registrations
and values made during the import, so `ChangeStore::rollback_import`, after
truncating and evicting blocks (see
[movable-list-op-validation.md](movable-list-op-validation.md)), drops the
parsed changes of every kept block that still has its encoded bytes; the next
access parses again and registers again. Blocks without bytes were built from
changes inserted before the import, so their containers were registered then.
Before 2026-09-28 the parsed ops kept indices and value slices past the
truncated arena: exporting the history hit `unreachable!` in the JSON encoder,
and a later checkout panicked on a missing value (loro-dev/loro#1161).

The resolver can run during any import, because it does not take the op log
lock. So every arena rollback of a failed import goes through the change store
and runs under the state lock:

- Under `inner`: `ChangeStore::rollback_arena` rolls the arena back and drops
  the parsed changes of blocks with bytes. `rollback_import` ends with it. The
  early returns of `LoroDoc::import_changes_and_apply_delta_to_state_if_needed`
  (a decode error, or updates that depend on history before the shallow root)
  call it through `OpLog::rollback_arena`; they used to call
  `SharedArena::rollback` directly. Blocks are parsed under `inner`, so a
  block the resolver loads is either dropped by the rollback or parsed against
  the rolled-back arena. A failed snapshot import replaces the change store:
  `ChangeStore::retire` rolls the arena back and makes the old store answer
  `Absent`, so a resolver that reached it before the swap does not register the
  discarded history.
- Under the state lock: `OpLog::rollback_import`, `rollback_arena`, and
  `reset_to_empty_for_failed_snapshot_import` take `&DocState` as proof. A
  state-locked query (`has_container`, `is_deleted`, `get_path_to_container`)
  can register containers through the resolver and then use their indices
  before it returns; the rollback must not free them in between. Without the
  state lock, a loom model hit `get_depth` on a freed index.

Only the state-locked queries are covered. A handler created on another thread
during the import keeps its index, and `DocState` is not rolled back: its store
and dead-container cache keep entries at the freed indices, and the next
container registered at one inherits them. `get_path_to_container` answers
`None` if its registration is already gone. See loro-dev/loro#1164
(pre-existing).

## Testing pitfall

Under `cfg(test)`, `ChangeStore::import_all` parses the last change of every
peer at load. Unit tests in `loro-internal` therefore do not see lazily parsed
blocks unless the change is not the peer's last; the `loro` crate's tests run
the real path.

## Tests

- `crates/loro/tests/unregistered_container_parent.rs`: repros for every entry
  point with snapshot, fork, and shallow sources, the dumped state from the
  review harness, the import rollbacks (including #1161's missing values), a
  thread stress of queries against history readers
  (`UNREGISTERED_PARENT_THREAD_TRIALS`), and a random comparison of every tree
  meta against a full-history import (`UNREGISTERED_PARENT_SEEDS=0..300`).
- `crates/loro/tests/unregistered_container_parent.rs`
  `queries_race_with_failing_imports`: queries against imports that fail at
  decode or depend on history before a shallow root, swept across the import's
  measured duration.
- `crates/loro/tests/multi_thread_test.rs` (run by `pnpm test-loom`): the lock
  order (`resolving_a_meta_parent_while_another_thread_reads_the_history`) and
  an import that fails at decode
  (`resolving_a_meta_parent_while_an_import_fails`), as loom models.
- Unit tests in `arena.rs` (the resolver runs outside the arena lock, the
  `None` / panic / cycle cases) and `parent.rs` (the local link check).
