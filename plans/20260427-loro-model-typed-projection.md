# Plan: Typed `loro_model` Layer with Projection and Reconcile

Date: 2026-04-27, revised 2026-10-16
Status: Draft. Phase 0 prototype done (`crates/loro-model`, `publish = false`).
Related issue: <https://github.com/loro-dev/loro/issues/888>
Compatibility stance: additive and opt-in; no changes to existing `loro` APIs.

## How to Use This Document

- Update phase status as work progresses: `Not Started`, `In Progress`,
  `Blocked`, or `Done`.
- A phase is `Done` only when its exit criteria hold.
- Public API shape changes go to the Decision Log first.
- Every behavioral claim below is backed by a test or benchmark in
  `crates/loro-model`; keep those in sync with this document.

## Summary

Issue #888 asks for autosurgeon-style conversion between Rust structs and Loro
documents. This plan provides that conversion plus a typed live model:

- **Data traits.** User types implement `FromLoro` (projection, `read_as`) and
  `ToLoro` (reconcile, `update_from`). Derive generates both.
- **Typed views.** Derive also generates a view per struct (`ProjectModel`,
  `TodoModel`) with field accessors. Views store *locations*, not data: every
  read goes to the current document state. There is no model cache, so there is
  no `pull()`, no route table, and nothing to keep coherent with import,
  checkout, undo or `revert_to`.
- **Keyed lists.** `Vec<T>` whose element type has `#[loro(key)]` is stored as a
  map of mergeable children plus an order list. Lookup by key is one map read,
  and two peers that create the same key concurrently merge into one element.
- **Watching.** `watch(|old, new| ..)` keeps only the last value of the watched
  subtree, so old values do not require a model-wide cache.

The prototype validates the end-to-end example below, minimal op emission,
convergence under randomized multi-peer edits, and flat per-operation cost on
the view path from 1k to 100k elements.

## End-to-End Example

This is the design's core fixture. It is a compiled, passing test:
`crates/loro-model/tests/model.rs::end_to_end_example`. The generated code it
relies on is hand-written in `crates/loro-model/tests/common/mod.rs` using only
what a derive macro can see.

### Define the data shape

```rust
use loro::{ExportMode, LoroDoc};
use loro_model::View; // read_as, update_from, edit, subscribe, watch

#[derive(Clone, Debug, PartialEq, LoroModel)]
struct Project {
    title: String,              // scalar string
    todos: Vec<Todo>,           // keyed vec, because Todo has #[loro(key)]
    #[loro(text)]
    notes: String,              // LoroText, edited with text ops
}

#[derive(Clone, Debug, PartialEq, LoroModel)]
struct Todo {
    #[loro(key)]
    id: String,
    title: String,
    done: bool,
}
```

`#[derive(LoroModel)]` generates `FromLoro`, `ToLoro`, `LoroSchema`, and a view
type named `ProjectModel` / `TodoModel` (overridable with
`#[loro(model = Name)]`). Plain structs stay the application's DTOs.

### Full reconcile and full projection

```rust
let doc = LoroDoc::new();
let project = ProjectModel::attach(doc.get_map("project"));

project.update_from(&initial)?;           // reconcile; reuses containers
let hydrated: Project = project.read_as()?; // one get_deep_value + convert
assert_eq!(hydrated, initial);
```

### Partial local updates through generated accessors

```rust
project.title().set("Typed model plan")?;           // one map op

let todo1 = project.todos().by_key("todo-1")?.unwrap(); // one map read
todo1.title().set("Write the typed model plan")?;
todo1.done().set(true)?;

project.notes().splice(0, 0, "Decision log\n")?;     // text op
assert_eq!(project.title().get()?, "Typed model plan"); // visible before commit
```

Views take `&self`: Loro handlers already have interior mutability, and a view
holds no cache that `&mut` would protect.

### Import: no refresh step

```rust
bob.import(&alice.export(ExportMode::Snapshot)?)?;
let bob_project = ProjectModel::attach(bob.get_map("project"));
let bob_todo1 = bob_project.todos().by_key("todo-1")?.unwrap();

// ... Alice inserts todo-0 at the front and renames todo-1; Bob imports ...

assert_eq!(bob_todo1.title().get()?, "Renamed by Alice");
```

`bob_todo1` is bound to the key `todo-1`, not to an index, so the front insert
does not redirect it.

### Watch a path with old and new values

```rust
let _sub = bob_todo1.title().watch(|old: &String, new: &String| {
    tracing::info!(%old, %new, "todo title changed");
})?;
```

Fires after commit, import, checkout, undo and `revert_to`, but only when the
watched value actually changed (unrelated fields are filtered by event path).

### Read a subtree as a custom struct

```rust
#[derive(FromLoro)]
struct TodoSummary { id: String, title: String }

let summaries: Vec<TodoSummary> = bob_project.todos().read_as()?;
let rows: Vec<FxHashMap<String, LoroValue>> = bob_project.todos().read_as()?;
```

`Vec<T>` projection accepts both list layouts, so a projection type does not
need to know whether the writer used a key.

### Read-modify-write

```rust
project.edit(|p: &mut Project| p.todos[0].done = true)?;
```

`update_from` with a DTO read earlier reverts anything that changed since that
read, including imported remote edits (test:
`stale_dto_reverts_remote_edits_but_edit_does_not`). `edit` reads first.

## Core Traits

Four traits, down from eight in the first draft.

```rust
/// Projection. `read_as` calls `read` under one `LoroDoc::read_state` lock;
/// `from_loro` converts an already materialized value.
pub trait FromLoro: Sized {
    fn from_loro(v: &LoroValue) -> Result<Self>;
    fn from_missing() -> Result<Self> { Err(missing) } // Option<T>, #[loro(default)]
    fn read<'r>(r: &mut StateReader<'r>, e: ReadEntry<'r>) -> Result<Self> {
        /* default: from_loro of the entry's deep value; derive reads field by field */
    }
}

/// Reconcile into a slot: map key, list position, new list element, or an
/// existing container.
pub trait ToLoro {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()>;
    const KEY_FIELD: Option<&'static str> = None; // #[loro(key)]
    fn key(&self) -> Option<&str> { None }   // string: a map key in the keyed layout
    /// Exact "to_loro would emit no ops" test under a read lock; false is always safe.
    fn same_as<'r>(&self, r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool { false }
}

/// Links a data type to its views, so derive output for `todos: Vec<Todo>`
/// can name `<Vec<Todo> as LoroSchema>::View` without seeing Todo's definition.
pub trait LoroSchema: FromLoro + ToLoro {
    type View: View;
    type VecView: View; // KeyedVec<Self> if keyed, else ListView<Self>
}

/// Typed view. Implemented by library views and generated XxxModel types.
pub trait View: Sized {
    fn from_loc(loc: Loc) -> Self;
    fn loc(&self) -> &Loc;
    // provided: read_as, update_from, edit, subscribe, watch
}
```

| First draft | Now |
| --- | --- |
| `ReadAs`, `UpdateFrom` method traits | Provided methods on `View` |
| `FromLoroModel` | `FromLoro` |
| `ToLoroModel`, `LoroReconcile` | `ToLoro` |
| `LoroHydrate`, `LoroPatch` | Gone: views do not cache |
| `LoroKeyed` | `ToLoro::KEY_FIELD` + `ToLoro::key` |
| (implicit) | `LoroSchema`: needed by derive to name nested view types |

`VecView` makes `by_key` exist only on keyed lists, at the type level.

## Views and Locations

```rust
pub enum Loc {
    Container(Container),        // roots, positional list elements
    Field(Arc<Loc>, Arc<str>),   // map field, resolved on each access
    KeyedItem(Arc<Loc>, Arc<str>), // keyed vec item; rejects keyed DTO renames
}
```

- A field view resolves its parent chain on each access: `O(depth)` map reads.
  It never caches a child handle, so a field that another peer replaced is
  observed instead of leaving a stale handle.
- Reads never create containers. Writes create missing parents as mergeable
  children.
- `by_key` returns `Loc::KeyedItem(items, key)`: bound to the element's identity.
  Reconciling a keyed DTO with a different key (including through `edit`) returns
  `KeyMismatch` before writing. Low-level `Slot` and handler access remain explicit
  escape hatches, as does writing arbitrary unkeyed shapes through `update_from`.
- A positional list element view is bound to the element container and returns
  `ErrorKind::Deleted` once that element is deleted.

## Storage Layout

| Rust type | Loro representation |
| --- | --- |
| struct | map; container-valued fields are mergeable children |
| `String`, `bool`, ints, `f64`, `Option<T>` | value (`None` deletes the key) |
| `#[loro(text)] String` | mergeable `LoroText`, updated with `LoroText::update` (diff) |
| `#[loro(counter)] f64` | mergeable `LoroCounter`, incremented by the delta |
| `Vec<T>`, `T` unkeyed | `LoroList`, reconciled by position |
| `Vec<T>`, `T` keyed | keyed vec: `{ items: Map<key, T>, order: MovableList<key> }` |
| `HashMap/FxHashMap/BTreeMap<String, V>` | map; keys missing from input are deleted |

Integers are stored as `i64`; `u64`/`usize` above `i64::MAX` return
`OutOfRange` on write, and out-of-range reads return `OutOfRange`.

### Why mergeable children

If two peers run `update_from(&initial)` offline on empty documents, regular
`insert_container` makes each create its own `todos` list, and map LWW keeps
only one: the other peer's items are lost (test:
`baseline_regular_containers_lose_concurrent_init`). With
`LoroMap::ensure_mergeable_*` both write the same deterministic child, and both
peers' items survive (test:
`concurrent_init_merges_instead_of_dropping_a_subtree`). See
`context/mergeable-containers.md`.

### Keyed vec

```text
todos: Map {
  items: Map { <key>: <element map> }   // each element is a mergeable child
  order: MovableList [<key>, ...]       // scalar keys, order only
}
```

Rules:

- `items` decides membership; `order` only orders. `len()`, `contains_key()` and
  `by_key()` read `items` only, so each is a single map read.
- Concurrent creation of the same key writes into the same deterministic child:
  the element's fields merge (LWW per field). It does not create a duplicate.
- Concurrent edits can leave `order` with a repeated key, a key whose item is
  gone, or no entry for a live item. The last case was found by the fuzz test:
  two peers dedupe a repeated key differently after a concurrent move, and
  together they delete both copies. Readers use the first occurrence, skip dead
  keys, and append unordered live items sorted by key. The next `update_from`
  writes that order back (test: `unordered_items_stay_visible`).
- Keys must be strings (they are map keys). Generated views expose key fields as
  read-only, because the key is the element's identity.
- `KeyedVec::insert` and `move_to` normalize `order` to the current visible order
  before using positions: remove repeated/dead keys and append missing live keys.
  Otherwise moving the first copy can expose another copy at its old position,
  and unordered items cannot serve as insertion anchors. Normalization does not
  read or rewrite item contents. Clean orders emit no repair ops; irregular orders
  use the existing `reconcile_order` pass (`O(n log n)` bookkeeping).
- Re-inserting a deleted key resurfaces the old mergeable child, then reconcile
  overwrites every known field. Fields unknown to this schema come back with it.

The first prototype stored a keyed vec as a movable list of maps. It was
replaced after measurement: `by_key` was an `O(n)` scan (117 ms at 100k), and
concurrent creation produced duplicate elements whose edits were later
discarded.

## Semantics

### `update_from`

- Unchanged values emit no ops. Scalars are compared before writing, text uses
  a diff, and counters write only the delta.
- Struct maps keep keys they do not know, so an older client does not erase a
  newer schema's fields (test: `unknown_fields_are_preserved`). Collection maps
  delete stale keys.
- Keyed vec reconcile: delete items not in the input; then reconcile `order`
  with a longest-increasing-subsequence pass. Elements in the LIS stay put,
  every other surviving key is moved once, and new keys are inserted. Finally
  reconcile each element's content in place.
- Measured op counts (`update_from_emits_minimal_ops`): 1 field change → 1 op;
  front insert → 5 (order entry, item marker, 3 fields); delete → 2;
  move last to front → 1; full reverse of n → n − 1 moves; one character of
  text → 1.
- A type mismatch at a slot replaces the slot. Invalid input returns `Err` with
  a path. The input `Vec` itself containing a duplicate key is
  `ErrorKind::DuplicateKey`.

### `read_as`

- Reads through one `read_state` lock, with field-level conversion: `O(output size)`.
  A struct's `read` override must handle `ReadEntry::Value` by calling `from_loro`;
  calling `FromLoro::read` there dispatches to that same override and recurses.
- Errors carry a path, built only on the error path:
  `todos[1].title: expected string, found I64(42)` (test: `errors_have_paths`).

## Subscriptions and Watch (issue 6)

`change.old()` in the first draft required a model cache, because Loro events
carry new values and deltas, not old values. The revision splits the API into
two methods:

- `subscribe(cb)`: raw Loro events for this subtree, no old values. Subscribe
  to the concrete `Loc::Container` anchor and filter the complete field path,
  including map diffs that replace or delete any ancestor edge. Do not bind to
  the currently resolved immediate parent: replacing it would orphan the subscription.
  Subscriptions can be created before intermediate maps exist. Event path segments
  name the child container plus its index in its parent; match field names *after*
  the anchor segment. Sibling edits do not fire; descendant edits do.
- `watch::<T>(|old, new| ..)`: keeps only the last projected `T`, re-reads the
  subtree on each relevant event, and calls back only when the value changed.
  The old value costs `O(size of T)` for that one watch, not a model-wide cache.

Both fire after commit. Views see uncommitted local edits immediately.

## Refresh Strategy (issue 5)

The first draft refreshed a model cache in `pull()` via
`doc.diff(cached_frontiers, state_frontiers)`. Problems with that:

- `diff` replays history between two versions. Its cost depends on the ops in
  between, and it returns diffs for the whole document, not just this subtree.
- Shallow snapshots cannot diff from frontiers before the shallow root.
- Import is only one of the ways state changes. Checkout, undo/redo,
  `revert_to`, `apply_diff` and raw-handler writes would each need their own
  invalidation path, and a missed one returns stale data silently.

Revised design:

1. **Default: no cache.** Views read current state, so `pull()` does not exist
   (test: `views_are_fresh_without_pull` covers commit, undo, redo, checkout,
   `checkout_to_latest` and `revert_to`).
2. **Derived values** that must stay current (old/new, memoized projections):
   `watch`, driven by subscription events. Events are emitted for every kind of
   state change, which is the property `pull()` lacked.
3. **Optional snapshot layer** (Phase 4, only if benchmarks show the need):
   an immutable `Arc` snapshot of the whole model for UI frameworks that want
   cheap clones and pointer-equality change detection. Proposed design:
   - Subscribe to the model root and record a dirty set of `ContainerID`s from
     each event's target and path.
   - On `snapshot()`, rebuild only the dirty containers and their ancestors by
     reading current state.
   - Reuse unchanged children from a `ContainerID → Arc<node>` table.
   - Re-reading current state instead of applying deltas avoids a second delta
     interpreter. Cost is `O(Σ size of dirty containers + depth)`; text and
     long lists can later switch to applying deltas if measurements justify it.

## Validation Results (Phase 0)

### Tests (`cargo test -p loro-model`)

| Test | What it proves |
| --- | --- |
| `end_to_end_example` | The example above, including import, keyed handle stability, and watch old/new |
| `update_from_emits_minimal_ops` | Op counts listed under `update_from` |
| `views_are_fresh_without_pull` | Correct reads and watch callbacks across commit, undo, redo, checkout and `revert_to` |
| `concurrent_init_merges_instead_of_dropping_a_subtree` / `baseline_regular_containers_lose_concurrent_init` | Need for mergeable children |
| `unordered_items_stay_visible` | Keyed vec membership rule |
| `errors_have_paths`, `unknown_fields_are_preserved`, `stale_dto_reverts_remote_edits_but_edit_does_not` | Error paths, forward compatibility, the stale-DTO hazard |
| `convergence::multi_peer_convergence` | 3 peers, random view edits, `update_from` of mutated projections, and random syncs. Checks: every peer converges; projections have no duplicate keys; view key order equals projection order; each view insert/move reaches its requested visible position; `update_from` round-trips and is idempotent at every step. The revised suite passes its default 100 seeds. The earlier suite without the insert/move assertions passed 5000 release seeds. |
| `regressions` | Plain map projection and malformed values without recursion; immutable keyed DTO identity; indexed edits after real concurrent creation and with repeated/dead/missing order entries; nested sibling filtering and descendant notifications; local/remote ancestor replacement, missing ancestors, checkout and deletion; unsubscription. |

### Benchmark (`cargo run --release -p loro-model --example model_bench`)

Single run on a shared 8-core Linux machine. The 100k column varies by up to
about 2× between runs. The flat rows and the ratios between rows are the signal.

| operation | n = 1k | n = 10k | n = 100k |
|---|---|---|---|
| view: `title().get()` | 419 ns | 415 ns | 411 ns |
| view: `title().set()` + commit | 3.0 µs | 3.2 µs | 7.7 µs |
| view: `by_key(last)` | 4.9 µs | 4.4 µs | 5.3 µs |
| view: held element `.done().set()` + commit | 10.1 µs | 11.0 µs | 11.0 µs |
| remote: import 1-field update | 13.4 µs | 13.2 µs | 14.9 µs |
| remote: import 1-field update + `watch` | 26.2 µs | 27.8 µs | 28.2 µs |
| `read_as::<Project>()` | 3.7 ms | 70.6 ms | 1007 ms |
| `update_from`: unchanged (0 ops) | 3.0 ms | 44.1 ms | 616 ms |
| `update_from`: 1 field changed | 3.1 ms | 45.1 ms | 607 ms |
| `update_from`: initial write | 31.9 ms | 358 ms | 3724 ms |
| baseline: raw write, list of maps | 7.4 ms | 88 ms | 976 ms |
| baseline: raw write, mergeable map of maps | 17.7 ms | 203 ms | 2144 ms |
| baseline: `get_deep_value()`, list of maps | 1.0 ms | 14.7 ms | 194 ms |
| baseline: `get_deep_value()`, mergeable map of maps | 2.4 ms | 51.8 ms | 704 ms |

Findings:

- The view path is flat in n: reads, writes, `by_key`, imports and watches.
  This is the hot path the design targets.
- `read_as` and `update_from` are `O(n)` boundary operations, as designed.
  `update_from` with no changes costs about 6 µs per element at 100k (handler
  reads for the comparison).
- The keyed-vec layout pays for mergeable children in Loro core: about 2.2×
  slower raw writes and 3.6× slower `get_deep_value` than a list of maps. In
  return, `by_key` and `len` are a single map read and concurrent creation of a
  key merges instead of duplicating.
- With the old movable-list-of-maps layout, `by_key(last)` took 0.9 ms, 11.5 ms
  and 117 ms at 1k, 10k and 100k, so it was replaced.
- The first keyed-reconcile bookkeeping was quadratic (7.8 s initial write at
  100k). It now uses a `Vec<u32>` simulation with anchor tracking and a fast
  path for an empty list.

## Performance Work (2026-10-15)

Profiled with an in-process sampler (`pprof`; `perf` is unavailable on the dev
machine) on 100k keyed todos. The model layer itself was a small share; the
time went to Loro core paths that every mergeable child pays:

| Path | Hot spot | Share |
| --- | --- | --- |
| initial write | `is_deleted` per op re-parses each mergeable ancestor's cid string and re-hashes its marker | 54% |
| `update_from` (unchanged) | `MapHandler::get` interns the key in the global interner; marker → cid translation rebuilds and interns the cid string | ~30% |
| `get_deep_value` | per child: marker CRC (bitwise, two `Vec` allocations), cid string build + intern, arena hash of the long cid | ~45% |

Changes (all in core, semantics unchanged; see
`context/mergeable-containers.md`, "Arena Edge Cache"):

- loro-common: table-driven, allocation-free marker CRC
  (`MergeableMarkerDigest`); a test pins it to the original bitwise CRC because
  the digest is stored data.
- `InternalString: Borrow<str>`; `MapState` looks keys up by `&str` without
  interning.
- The arena caches each mergeable cid's parsed edge and activating marker bytes,
  with a `(parent, key) -> child` reverse index. Liveness, map reads, deep
  values, the alive walk and `read_container_tree` use it.
- Model layer: keyed reconcile uses borrowed `&str` keys; `ToLoro::key()` returns
  `Option<&str>`.

A/B against the previous commit, same binary interleaved, minimum of 3–5 runs
each, 100k todos, shared 8-core machine:

| Operation | Before | After | Speedup |
| --- | --- | --- | --- |
| `update_from`, unchanged | 715 ms | 323 ms | 2.2× |
| `get_deep_value` | 950 ms | 528 ms | 1.8× |
| `read_as::<Project>()` | 1.18 s | 0.77 s | 1.5× |
| initial `update_from` | 3.68 s | 2.1–2.4 s | 1.6× |
| `by_key` | 4.4 µs | 1.6 µs | 2.7× |
| held element `set` + commit | 9.9 µs | 4.4 µs | 2.3× |
| first `get_deep_value` after snapshot import | 2.1 s | 2.0 s | — |

### Round 2: field-level state reads

New public API, `LoroDoc::read_state(|r: &mut StateReader| ...)`
(`crates/loro/src/state_reader.rs`, design and rules in
`context/state-reader.md`): one state lock, `map_get` / `list_entries` /
`value` / ... returning a value or a child `ContainerRef`, nothing
materialized beyond what is read. `ContainerRef<'r>` is branded with an
invariant lifetime, so refs cannot escape the closure or mix with another
call's reader (compile-fail doctests).

Model layer:

- `FromLoro::read`: derive reads struct fields directly; `Vec`, keyed vecs,
  maps, text and counter read through the reader. `read_as` and `watch` use it.
- `ToLoro::same_as`: exact "no ops needed" test. Keyed reconcile does one
  `read_state` pass (order, item keys only if something is stale, and
  `same_as` per element) and calls `to_loro` only for changed elements.
- Views capture their `LoroDoc` when created. `ContainerTrait::doc()` calls
  `start_auto_commit`, which takes the transaction lock that a committing
  thread holds while emitting events, so calling it from a subscriber panics
  (debug) or deadlocks (release). This affects any user code that calls
  `.doc()` in a subscriber; recorded in `context/state-reader.md`.

Core: `MovableListState::iter` walked the list with one tree query per index
(`O(n log n)`, marked `PERF`); it is now one in-order walk.

A/B against the previous commit, thread CPU time, minimum of 3–7 runs, 100k
todos, quiet machine:

| Operation | Before | After | Speedup |
| --- | --- | --- | --- |
| `read_as::<Project>()` | 747 ms | 371 ms | 2.0× |
| first `read_as` after snapshot import | 2.04 s | 1.24 s | 1.65× |
| `update_from`, unchanged | 315 ms | 285 ms | 1.1× |

Cumulative since the start of the performance work: `read_as` 1.18 s → 0.37 s
(3.2×), `update_from` unchanged 715 ms → 285 ms (2.5×).

### Where the remaining time goes

Per-operation microbenchmarks inside one `read_state`, 100k keyed todos:

| Read | Cost |
| --- | --- |
| `map_get` on the 100k-entry `items` map (incl. mergeable child lookup) | ~860–990 ns |
| three `map_get`s on one todo's 3-entry map | ~610–680 ns |
| `list_entries` on `order`, per element | ~190 ns |
| `map_keys` on `items`, per key | ~150 ns |

These are memory-latency bound, not instruction bound: the profile of the
`items` lookup is two hash probes (`MapState`, then the arena's
`(parent, key) -> child` index), each dominated by loading control bytes and
dereferencing the stored `InternalString` (`Arc<Box<str>>`, two hops) on
scattered memory. The lock that `read_state` removes was not the main cost,
which is why `update_from` gained little.

Tried and dropped: replacing the large-map `BTreeMap` with a hash map plus a
lazily sorted key list (keeps encoding byte-identical). Lookups went
992 → 863 ns, but key iteration went 146 → 255 ns and the change touched
snapshot encoding paths. Not worth it.

Options that would move the floor, each a core data-layout change:

- `InternalString` with a thin `Arc` holding the bytes inline (one hop instead
  of two). `Arc<str>` is a fat pointer and would grow every `InternalString`
  from 16 to 24 bytes.
- Store the resolved child index next to a mergeable marker in `MapState`, so
  reading a mergeable child is one probe instead of two.
- First read after import: each container is decoded from KV on first touch,
  and the decoded-value cache (bounded at 2048, `context/container-value-cache.md`)
  evicts and drops most of them during a 100k-container read (~26%);
  registering 100k mergeable cids, half of it interning keys, is ~21%; KV point
  lookups ~9%.

## Known Costs and Follow-Ups

- **Mergeable children in core**: before the work above, about 2.2× write and
  3.6× `get_deep_value` cost versus regular children. Re-measure the baselines
  in `model_bench` after the changes.
- **Index-based keyed operations** (`insert` at an index, `move_to`, `remove`,
  `position_of`) scan `order`: `O(n)` scalar reads. A core API for movable-list
  element position by element id, or an order-statistic index, would make them
  `O(log n)`.
- **Layout visibility**: JS and other non-Rust readers see
  `{ items, order }` for keyed vecs. Document the layout. A `loro-crdt` helper
  can come with WASM support.
- **Watch cost**: each relevant event re-reads the watched subtree. Watching a
  large subtree is `O(subtree)` per event; recommend watching narrow paths.

## Derive Macro Contract

The macro sees only the annotated item, so it may rely only on:

- field names, field types and `#[loro(...)]` attributes;
- `<FieldType as LoroSchema>::View` for nested views;
- `ToLoro::KEY_FIELD` of element types, which selects the keyed layout at
  runtime and `VecView` at the type level.

Attributes:

| Attribute | Expansion |
| --- | --- |
| `#[loro(key)]` | `ToLoro::KEY_FIELD`, `ToLoro::key`, `type VecView = KeyedVec<Self>`, read-only view accessor |
| `#[loro(text)]` | sugar for `#[loro(with = "loro_model::text")]`; view type `TextField` |
| `#[loro(counter)]` | sugar for `#[loro(with = "loro_model::counter")]` |
| `#[loro(with = "module")]` | `module::from_loro` / `module::to_loro` |
| `#[loro(rename = "...")]` | map key |
| `#[loro(default)]`, `#[loro(missing = "fn")]` | `from_missing` override |
| `#[loro(model = Name)]` | view type name; default `{Type}Model` |

Per struct, the macro emits: `FromLoro`, `ToLoro`, `LoroSchema`, a view struct
holding a `Loc`, `impl View`, `attach(LoroMap)`, and one accessor per field. The
expansion in `tests/common/mod.rs` is the reference for what to generate.
`#[derive(FromLoro)]` alone (projection-only DTOs such as `TodoSummary`) emits
only `FromLoro`.

## Crate Layout

- `crates/loro-model`: runtime (prototype is here; `publish = false` until
  Phase 1).
- `crates/loro-model-derive`: proc macro (Phase 2).
- Later: `loro = { features = ["model"] }` re-export, once the API is stable.

## Phases

### Phase 0: Prototype and API validation — Done

- [x] Runtime: traits, `Slot`, `Loc`, views, keyed vec, positional list, std
      impls, text, counter.
- [x] Hand-written derive expansion for the example schema.
- [x] Behavior tests, randomized convergence test, benchmark.
- [x] Plan revised from measured results.

### Phase 1: Runtime hardening — Not Started

- [ ] Unit tests for every std impl (`Option`, ints at boundaries, `f64`,
      `BTreeMap`, `HashMap`, counter).
- [ ] Nested struct views (`Loc::Field` chains deeper than 2) and `Option<Struct>`
      views. Today `Option<T>` has only a `ValueField` view.
- [ ] `MapView<V>` for `HashMap<String, V>` fields with `get(key)`/`insert`/`remove`.
- [ ] `ListView::mov`. Plain `LoroList` has no move op; `ListRef::mov` falls
      back to delete + insert, which loses element identity. Decide whether
      positional lists of structs should use a movable list instead.
- [ ] Extend the fuzz test: checkout/undo between steps, text edits through
      views, nested structs.
- Exit: all of the above tested; fuzz passes 5000 seeds in release.

### Phase 2: Derive macro — Not Started

- [ ] `crates/loro-model-derive`: `#[derive(LoroModel)]`, `#[derive(FromLoro)]`.
- [ ] Attributes in the table above; compile errors for unsupported shapes
      (tuple structs, generics) via `trybuild`.
- [ ] Replace `tests/common/mod.rs` with derived code; existing tests must pass
      unchanged.
- [ ] Enums: externally tagged map (`{ "kind": .., ...fields }`); design note
      before implementation.
- Exit: example compiles from the derive alone.

### Phase 3: Performance — In Progress

Gate each item on the benchmark; keep the tables above updated.

- [x] Core: mergeable edge cache, table CRC, `&str` map lookups
      (see "Performance Work").
- [x] Core: non-materializing state reader (`LoroDoc::read_state`), used by
      `read_as` and keyed reconcile.
- [x] Core: `MovableListState::iter` in one walk.
- [ ] Core data layout: thin `InternalString`, child index next to mergeable
      markers (see "Where the remaining time goes").
- [ ] Core: first read after import (decode/evict churn, cid registration).
- [ ] Keyed index-based operations: `O(log n)` position of an `order` entry.
- Exit: `update_from` unchanged at 100k within 2× of `get_deep_value()` of the
  same data, or a documented reason why not.

### Phase 4: Snapshot layer — Not Started, optional

Only if a user scenario needs whole-model immutable snapshots. Design under
"Refresh Strategy". Exit: snapshot update cost proportional to dirty
containers, with a benchmark.

### Phase 5: Public integration — Not Started

- [ ] Crate docs: view vs projection vs reconcile, costs, failure modes,
      the stale-DTO hazard, commit timing of `subscribe`/`watch`.
- [ ] Decide the `loro` feature re-export.
- [ ] WASM: decide whether `loro-crdt` exposes keyed-vec helpers; if it adds
      bindings, audit the pending-event flush allowlist
      (`crates/loro-wasm/AGENTS.md`).
- [ ] Changeset when anything publishes.

## Open Questions

- Should `update_from` on a keyed vec with a non-string key type be a compile
  error (a `LoroKey` bound) rather than a runtime `Err`?
- Indexed view writes now repair `order` before editing. Should `remove` also
  normalize unrelated repeated/dead/missing keys, beyond removing its own key?
- Should `watch` accept a custom comparator for `T` without `PartialEq`?
- Positional `Vec<Struct>` (no key) is reconciled by index, so inserting at the
  front rewrites every element. Should derive warn when a `Vec` of structs has
  no key?

## Decision Log

- 2026-04-27: Prefer first-party model types over making plain user structs
  the primary live model.
- 2026-04-27: `read_as` / `update_from` naming; avoid `cast` and `try_set`.
- 2026-04-27: Derive generates concrete view types (`ProjectModel`), because a
  user crate cannot add inherent methods to a foreign generic type.
- 2026-10-15: Views store locations, not data. No model cache, no `pull()`, no
  route table. Supersedes the cached `LoroModel<T>` with `LoroPatch` and
  `doc.diff`-based refresh.
- 2026-10-15: Four traits (`FromLoro`, `ToLoro`, `LoroSchema`, `View`) replace
  eight. `ReadAs`/`UpdateFrom` become provided methods on `View`.
- 2026-10-15: Struct fields that hold containers are mergeable children, so
  concurrent initialization does not drop a subtree.
- 2026-10-15: Keyed `Vec<T>` uses the `{ items, order }` layout; `items`
  decides membership. Replaces the movable list of maps after measuring `O(n)`
  `by_key` and duplicate elements on concurrent create.
- 2026-10-15: Old/new values come from `watch` (per-watch last value), not
  from events or a model cache.
- 2026-10-15: `Vec<T>` projection accepts both list layouts; `FromLoro` has no
  key concept.
- 2026-10-15: `FromLoro::read` / `ToLoro::same_as` over `LoroDoc::read_state`;
  views capture their `LoroDoc` instead of calling `ContainerTrait::doc()`.
- 2026-10-15: `ToLoro::key()` returns `Option<&str>`: keys are map keys in the
  keyed vec layout, and borrowing avoids an allocation per element.
- 2026-10-16: Indexed keyed-vec edits normalize the visible order first;
  `KeyedItem` rejects keyed DTO identity changes. Subscriptions anchor at the
  concrete container and filter the full logical path so ancestor replacements
  do not orphan watches. Regression tests cover these outcomes, not only convergence.
