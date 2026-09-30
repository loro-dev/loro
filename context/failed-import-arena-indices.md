# Failed Imports Keep Their Arena Indices

Verified against code 2026-09-30.

A failed import rolls the op log back (`OpLog::rollback_import`, `rollback_arena`,
`reset_to_empty_for_failed_snapshot_import`), including the arena
(`SharedArena::rollback`, always called through the change store; see
[arena-parent-links.md](arena-parent-links.md#import-rollback)). The arena rollback
keeps every container index. It used to truncate the containers registered since the
checkpoint and hand their indices to the next registrations (loro-dev/loro#1164).

## Why indices are not freed

Much outside the op log is keyed by `ContainerIdx` and is not rolled back:

- `DocState`'s store (`InnerStore` wrappers and its decoded-value cache). A failed
  import loads wrappers from KV while it validates (`validate_diff_batch`,
  `needs_checkout_diff` → `store.get_container`).
- `DocState::dead_containers_cache` and `alive_containers_cache`.
- Handlers (`BasicHandler` holds the index), which another thread can create while
  the import runs: `get_map("x")`, tree metas, `get_container`.

When an index was reused, the new container inherited the old one's state. The issue's
repro shows a new root map `fresh` with the text of a tree meta that the failed import
had loaded at that index. Retrying the bad update tripped a type assertion in
`InnerStore` (a Map index holding Text state). With other threads, containers created
later read as deleted, and a container whose index was reused could become its own
ancestor and overflow the stack.

The other direction the issue suggested, rolling `DocState` back at indices at or above
the checkpoint, does not cover handlers. A handler another thread created during the
import would still write to whatever container got its index next. Keeping the
indices makes every index-keyed structure stay valid without tracking each one.

Keeping them is safe because a failed import changes no container state:
`DocState::apply_diff` validates the whole batch before it mutates anything, and
decode or DAG failures return before state is touched. A wrapper loaded while
validating holds that container's state at the kept version. Nothing is shown for a
root that the failed import registered but that has no store entry:
`preferred_root_containers` and `existing_retention_roots` filter by
`store.contains_id`.

## What the rollback still drops

- **Parent links** of normal containers registered since the checkpoint, and links from
  a normal container to such a container. They may come from the rolled-back changes,
  and a container whose only creating op was rolled back must not look alive
  (`has_container` and `is_deleted` answer as if it was never registered). Links that
  are true for the kept history come back through the resolvers: the state KV, or the
  op log's creator resolver, which parses the block again because
  `ChangeStore::rollback_arena` dropped the parsed changes of blocks parsed since the
  checkpoint (`SharedArenaRollback::keeps`). Root links are kept, since a root's ID
  determines them (none for a top-level root, the encoded parent for a mergeable root).
- **Depth cache.** When anything was registered since the checkpoint, every cached depth
  is reset: one could have been computed through a dropped link, including for the
  descendants of an older container. Kept links give the same depth again.
- **Values and text** allocated since the checkpoint are truncated. Only parsed changes
  refer to them, and those are dropped with the blocks above.
- **Dead-container cache.** `DocState::forget_parent_link_caches_after_failed_import`
  clears it on every rollback path (the `OpLog` rollback methods take `&mut DocState`
  and call it). A deletion cached through a link the rollback drops is not true of the
  kept history. It is a cache, so clearing only costs recomputation.

Local transactions cannot register containers during an import: `import`,
`import_json_updates`, and `import_batch` hold the transaction lock
(`with_barrier` / `BatchImportGuard`), and `checkout` needs the op log lock.

## Costs and limits

- Successful imports are unchanged; `checkpoint_for_rollback` reads two fewer lengths.
  `crates/examples/examples/failed_import_rollback_perf.rs` measured the same times
  before and after for 5k/20k small updates one by one, a large update into a loaded doc,
  and a failed large import (2026-09-30).
- A failed import leaves its container IDs registered. Retrying the same update
  registers nothing new; different failing updates grow the arena by the containers
  they name, like handlers for IDs that do not exist.
- A link the failed import set on a container registered *before* the checkpoint, to a
  parent also registered before it, is kept (pre-existing; it needs a journal of
  `set_parent` calls during the import). This takes a container that was registered
  without a parent, for example a handler for an ID not received yet, whose creating op
  is in the failed import.

## Tests

`crates/loro/tests/failed_import_state_rollback.rs`: the issue's repro verbatim, new
roots and children after two failures (plus snapshot and update round trips), retrying
the same bad update through `import` and `import_batch`, the valid update arriving after
the forged one failed, and a thread that creates root handlers and reads a tree meta while
the import fails twice (`FAILED_IMPORT_THREAD_TRIALS`, default 8). Every one except
the valid-update test failed before the fix. The threaded one failed in 10 of 10
single-trial runs.

`crates/loro-wasm/tests/failed_import_state_rollback.test.ts`: the same shape through
the JS API. It revives 40 nodes because which diffs the state validates before the
rejected one follows `FxHashMap` order, which differs on wasm32: with one or eight
nodes the old code happened to validate the text first and the test passed.
