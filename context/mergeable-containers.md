# Mergeable Container Context

Verified against code 2026-09-28.

Mergeable containers let two peers independently create the same child container
under a map key and converge to one deterministic container id. The source of
truth for visibility is a binary marker in the parent map slot, not whether the
child already has direct operations.

## Two-Hop Answer

If an agent asks "how do mergeable containers work?", start here:

- [crates/loro-common/src/lib.rs](../crates/loro-common/src/lib.rs):
  `MERGEABLE_NAMESPACE_PREFIX`, `ContainerID::new_mergeable`,
  `ContainerID::parse_mergeable`, `mergeable_marker`,
  `parse_mergeable_marker`, `translate_mergeable_marker_value`.
- [crates/loro-internal/src/handler.rs](../crates/loro-internal/src/handler.rs):
  `MapHandler::ensure_mergeable_container` and public
  `ensure_mergeable_*` helpers.
- [crates/loro-internal/src/state/mergeable.rs](../crates/loro-internal/src/state/mergeable.rs):
  logical child edge resolution from deterministic cid plus parent marker.
- [crates/loro-internal/src/state/map_state.rs](../crates/loro-internal/src/state/map_state.rs)
  and [crates/loro-internal/src/txn.rs](../crates/loro-internal/src/txn.rs):
  marker-to-container translation at read, diff, and event boundaries.
- [crates/loro-internal/docs/mergeable-container-id.md](../crates/loro-internal/docs/mergeable-container-id.md):
  current mergeable cid encoding.
- [crates/loro-internal/tests/mergeable_container/](../crates/loro-internal/tests/mergeable_container/)
  and [crates/loro-internal/tests/mergeable_cid_encoding.rs](../crates/loro-internal/tests/mergeable_cid_encoding.rs):
  regression coverage.

## Model

`MapHandler::ensure_mergeable_<kind>(key)` does two things:

1. Derives a deterministic `ContainerID::Root` with
   `ContainerID::new_mergeable(parent, key, kind)`.
2. Writes `mergeable_marker(parent, key, kind)` into the parent map slot.

The deterministic cid uses the reserved `🤝:` namespace. Its payload encodes the
nearest non-mergeable map ancestor and escaped key path. The child kind is stored
in `ContainerID::Root.container_type`, not duplicated in the root-name payload.

The marker is compact binary storage:

- magic bytes from `MERGEABLE_MARKER_MAGIC`,
- one byte for container kind,
- a 24-bit digest bound to `(parent, key, kind)`.

Copying marker bytes to another key or parent does not activate a mergeable child
there.

## Visibility And Conflicts

The parent map's current value decides visibility:

- no marker: child is hidden, though state may still exist at its deterministic cid;
- same-kind marker: child is active and read surfaces translate it to
  `LoroValue::Container`;
- different-kind marker: parent map LWW picks the visible kind.

Concurrent same-kind creation writes identical markers and merges into the same
child. Concurrent different-kind creation writes different markers; regular map
LWW chooses one visible kind. Losing-kind state must remain addressable by
deterministic cid and can resurface if a later `ensure_mergeable_<loser_kind>`
rewrites the marker.

## Boundaries

- User strings, arbitrary binary values, scalars, and regular child containers
  are not mergeable markers. `ensure_mergeable_*` must return `ArgErr` rather
  than overwrite them.
- Repeating same-kind `ensure_mergeable_*` over the same marker is idempotent and
  should not emit another op.
- Calling a different-kind `ensure_mergeable_*` over an existing mergeable marker
  is a deliberate local kind change.
- Deleting the map key clears the marker and hides the child; re-ensuring writes
  a new marker and resurfaces preserved state.
- Detached map handlers cannot ensure mergeable children, because the
  deterministic child cid depends on the attached parent cid.

## Revert, Apply-Diff, Events, And Undo

A mergeable child that becomes visible again keeps its deterministic cid and its hidden
state. What a diff consumer needs depends on whether it holds that hidden state:

- the doc that kept it (its own revert/undo, or a mirror that shares its history) needs
  the **actual change**;
- a doc without it (a fresh doc, a JSON mirror) needs the **full state**;
- a doc with a *different* hidden state needs the full state **aligned** with what it has.

A batch alone cannot say which it holds. In JS, `[cid, diff][]` has no room for that. For a
plain re-ensure, "no child entry" means "unchanged" in a local event but "empty" in a
full-state diff. So the mode is explicit:

- **Events keep main's shapes.**
  - Local transactions report the parent marker plus the transaction's own ops. A plain
    `ensure_mergeable_*` over a deleted key emits only the marker.
  - Import/checkout revival reports full state, as on main. It also records the actual
    change as `KeptChange` (`event.rs`: `InternalContainerDiff`/`ContainerDiff::kept`,
    `ContainerDiff::change()`). `DocState::apply_diff` tracks the `kept` set: a mergeable
    child re-activated by a container that is not itself revived, plus what a kept
    container holds, minus containers its own change inserts. A mergeable child under a
    parent revived under a fresh id is not kept, because `apply_diff` gives it a fresh cid.
- **`LoroDoc::diff`** reports full state and sets `DiffBatch::full_state`
  (`loro::event::DiffBatch::is_full_state`/`set_full_state`).
- **`apply_diff` is incremental by default**, as on main. It aligns only for a
  `full_state` batch (WASM: `applyDiff(diff, { fullState: true })`). In `_apply_diff` with
  `align_revived_mergeable`, a re-activated mergeable child's full state is aligned with
  what the doc holds at that cid (`handler/full_state.rs`):
  - Map: per key.
  - Counter: adds the difference.
  - Text: keeps the common prefix/suffix by content; re-marks kept chars whose style
    differs (`null` unmarks); replaces the middle.
  - List/MovableList: keeps the common prefix/suffix (values; containers by id) and
    replaces the middle. In a movable list, `apply_delta` turns deleting and re-inserting
    a child into a move (`movable_list_apply_delta.rs`, #1138), so middle children present
    in both keep their id and are full-state targets too. A plain list recreates them.
  - Tree: by `TreeID` (create/move/delete).
  - Kept children become further full-state targets. A target with no entry is cleared.
- **Revert and undo** apply `DiffBatch::from_changes` without alignment, recorded with
  `record_changes_only` (an optimization that skips unused full states). Only the real
  difference is written, keeping char/element/`TreeID` identity.
- **UndoManager** composes remote events with `ContainerDiff::change()`, so a peer can still
  undo its edit after a remote undo re-activates the child.

Which batches may carry the flag:

- Results of `diff()` do, including after a JSON round trip with `{ fullState: true }`.
- Import/checkout events may, because they revive children with full state.
- **Never** batches built from local events. A local re-ensure has no child entry because
  the child is unchanged; read as a full state, that means "empty" and clears it (a counter
  at 7 becomes 0).
- `DiffBatch::compose` (internal) keeps the flag. An empty side is the identity, and two
  full-state batches stay full-state: a child that the second batch re-activates was hidden
  where they meet, so the first has no entry for it. Mixing a full-state and an
  incremental batch panics, because neither mode can apply the result. `clear` resets the
  flag.

When to opt in:

- **Mirroring events** (subscribe → `applyDiff`) requires the receiver to share the
  source's hidden state, as on main. Use the default. To build a mirror from scratch, use
  `diff()` or a snapshot.
- **Applying `diff()` to a doc that may hold hidden state** for a child the diff
  re-activates (the same doc, a fork, a doc with another history) needs
  `{ fullState: true }` in JS. In Rust the flag comes with `LoroDoc::diff`. Without it, such
  a doc gets main's behavior: the full state is appended to the hidden one. On a doc
  without hidden state both modes give the same result.

Known limitations:

- Alignment is content-based and each receiver writes its own ops. Two receivers that each
  align the same full-state batch against the same hidden state and then merge can apply
  it twice (e.g. `hello` over `hXYZo` gives `hellello`, `[keep]` over `[other]` gives two
  `keep`, and two new tree nodes). That converges, but it is not idempotent. To restore
  once across peers, have one peer apply and sync the updates.
- Identity is kept for the shared prefix/suffix, moved movable-list children, and tree
  nodes by `TreeID`. Plain-list middle children and middle text are recreated.
- Counters stay additive: two peers reverting 10 -> 7 concurrently give 4, as for a root
  counter.
- `revert_to` treats a remote edit to the hidden child like a root container does: already
  observed means reverted, arriving later means merged. Undo of a delete keeps such edits
  in either order.

Covered by `crates/loro/tests/mergeable_revert.rs`, `tests/mergeable_container/delete.rs`,
and `crates/loro-wasm/tests/mergeable.test.ts`.

## Snapshot And Retention Rules

Snapshot and shallow snapshot export must preserve mergeable child state even
when that child is hidden by a different winning marker (full export keeps every
flushed KV entry; the shallow alive walk retains them explicitly). An
ensured-but-empty mergeable child has no KV entry of its own: full export omits
it and importers resolve it from the parent map's marker. This is covered by
`tests/mergeable_container/snapshot.rs`, including shallow snapshot tests for
losing-kind state. loro.js follows the same rule in `#retainedContainerKeys`
(`loro-js/tests/shallow-snapshot-deleted-containers.test.ts`).

Raw marker bytes are the wire/storage representation. Public read and diff
surfaces should translate an active marker to a container value. APIs that expose
raw/shallow storage may still show the binary marker for forward compatibility.

## Tests By Question

- Deterministic cid and malformed parser cases:
  `cargo test -p loro-internal --test mergeable_cid_encoding`
- Marker layout, idempotency, kind changes, and non-mergeable occupant guards:
  `cargo test -p loro-internal --test mergeable_container discriminator`
- Same-kind convergence and nested chains:
  `cargo test -p loro-internal --test mergeable_container convergence`
- Delete/hide/reactivate behavior:
  `cargo test -p loro-internal --test mergeable_container delete`
- Revert / diff + apply_diff / undo / checkout after delete:
  `cargo test -p loro --test mergeable_revert`
- Different-kind conflicts:
  `cargo test -p loro-internal --test mergeable_container type_conflict`
- Snapshot and shallow snapshot retention:
  `cargo test -p loro-internal --test mergeable_container snapshot`
- Pending import ordering:
  `cargo test -p loro-internal --test mergeable_container pending`
- Events and paths:
  `cargo test -p loro-internal --test mergeable_container events_and_paths`

## Common Misconceptions

- "A mergeable child is visible once it has ops." False; visibility is controlled
  by the parent marker.
- "Deleting the key deletes the child state." False; it hides the child by
  removing the marker.
- "A full-state revival of a re-activated mergeable child is its change." False; it is
  what a consumer that never saw the child needs. The doc that kept the child uses
  `KeptChange` (revert, undo, undo transforms), and `apply_diff` aligns a full state with
  the target's hidden state only for `full_state` batches.
- "Kind conflict discards the loser." False; the loser is hidden but should stay
  recoverable by deterministic cid.
- "The marker is the child cid." False; the marker activates a kind at a
  `(parent, key)`, while the cid is derived independently.
