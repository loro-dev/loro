# State Reader (`LoroDoc::read_state`)

Verified against code 2026-10-15.

## What it is

`LoroDoc::read_state(|r: &mut StateReader| ...)` (`crates/loro/src/state_reader.rs`,
over `crates/loro-internal/src/state/reader.rs`) holds the document's state lock once
and reads field by field:

- `map_get`, `map_len`, `map_keys`, `list_get`, `list_len`, `list_entries`, `value`
  (shallow: Text as string, Counter as number), `deep_value`, `is_deleted`.
- A read returns `ReadEntry::Value(LoroValue)` (never `LoroValue::Container`) or
  `ReadEntry::Container(ContainerRef)`, which is read further through the same reader.
- `container(&handle)` / `container_by_id(&id)` turn a handle or id into a `ContainerRef`.
  `container` returns `None` for a detached handle or another document's handle
  (`BasicHandler::is_in_arena`).

## Why

`get_deep_value` builds a `LoroValue` tree: one `HashMap<String, LoroValue>` per map
(`MapState::to_map`), which the caller converts and frees. Profiling `loro-model`'s
`read_as` on 100k map children showed materializing and dropping that tree at ~60% of
`get_deep_value`, and converting it at another ~30%. The reader goes through
`ContainerStore::{map_get, list_values, ...}`, which clone one value at a time. Results:
`plans/20260427-loro-model-typed-projection.md`, "Performance Work".

## Rules

- Same semantics as handler reads: uncommitted local changes are visible, and mergeable
  markers resolve to their child through the arena's edge cache
  (`SharedArena::mergeable_child_idx`; see `mergeable-containers.md`). A marker copied to
  another key stays a `Binary` value.
- `ContainerRef<'r>` carries an invariant brand lifetime and `read_state` takes
  `for<'r> FnOnce(&mut StateReader<'r>)`, so refs cannot leave the closure or mix with
  another call's reader. That is what makes a bare `ContainerIdx` inside safe.
- The closure must not call into the document (handlers, commit, import): the state lock
  is held. Debug builds panic on the lock-order check; release builds deadlock.
- Do not call `ContainerTrait::doc()` from an event subscriber: it calls
  `start_auto_commit`, which takes the transaction lock that the committing thread already
  holds while emitting events (debug: lock-order panic; release: deadlock). `loro-model`
  captures the `LoroDoc` when a view is created instead (`Loc::root`).

## Tests

- `crates/loro/tests/state_reader.rs`: reader-rebuilt values equal `get_deep_value`
  (regular, nested mergeable, lists, text, counter, tree, snapshot-loaded docs, random
  two-peer docs), copied markers, per-document refs, deleted containers.
- `crates/loro-model/tests/convergence.rs`: `read_as` through the reader equals
  converting the deep value, and `same_as` is exact (true ⇒ reconcile emits no ops).
