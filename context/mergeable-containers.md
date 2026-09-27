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

## Revert, Apply-Diff, And Undo

When a container becomes reachable again, `DocState::apply_diff` (`state.rs`)
normally records its *full* state as a from-empty diff. This is called revival, and it
is what checkout/import subscribers need. `LoroDoc::_apply_diff` recreates regular
containers under fresh ids, so full state is right for them. A mergeable child is
different: it is re-activated at its deterministic cid, and a doc at the diff's start
version still holds its hidden state there. Applying a full state on top used to
duplicate it (`"hello"` -> `"hellohello"`, counter 7 -> 14).

`LoroDoc::diff` and the undo diff closure record with
`DocState::mergeable_revival_as_delta` set. In that mode, a mergeable child that is
re-activated by the diff of a container that keeps its id (not itself revived) is not
revived. Its own calculator delta, or nothing when it has no ops in range, becomes the
batch entry. `_apply_diff` stays purely incremental. So:

- revert/diff/undo keep char, element and `TreeID` identity. Only the real difference is
  written; hidden content that already matches costs only the parent marker op.
- Concurrent reverts of the same state delete the same elements and merge to the
  target. Concurrent edits to kept elements (e.g. a tree node's meta) survive.
- Local events of a revert are increments on the hidden state. Forwarding them to a doc
  in the same state through `apply_diff` reproduces the source state.
- A mergeable child whose parent *is* revived under a fresh id (e.g. a map in a deleted
  list element) resolves to a fresh deterministic cid, so it still gets full state.

Consequences:

- `LoroDoc::diff` output for a re-activated mergeable child is a delta relative to its
  hidden state, not a mirror-friendly full state. Checkout/import events are unchanged
  and still revive with full state.
- `revert_to` behaves like it does for a root container: a remote edit to the hidden
  child that was already observed is reverted, and one that arrives later merges.
- Undo of a delete only restores the marker, so remote edits made while the child was
  hidden survive in either arrival order.
- Counters stay additive: two peers reverting 10 -> 7 concurrently give 4, as for a
  root counter.

Covered by `crates/loro/tests/mergeable_revert.rs` (including event forwarding, concurrent
reverts, and tree meta edits), `tests/mergeable_container/delete.rs`, and
`crates/loro-wasm/tests/mergeable.test.ts`.

## Snapshot And Retention Rules

Snapshot and shallow snapshot export must preserve mergeable child state even
when that child is hidden by a different winning marker (full export keeps every
flushed KV entry; the shallow alive walk retains them explicitly). An
ensured-but-empty mergeable child has no KV entry of its own: full export omits
it and importers resolve it from the parent map's marker. This is covered by
`tests/mergeable_container/snapshot.rs`, including shallow snapshot tests for
losing-kind state.

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
- "A re-activated mergeable child needs a full-state revival in a diff that will be
  applied to a Loro doc." False; the target doc still holds the hidden state, so
  `LoroDoc::diff`/undo record the child's delta instead.
- "Kind conflict discards the loser." False; the loser is hidden but should stay
  recoverable by deterministic cid.
- "The marker is the child cid." False; the marker activates a kind at a
  `(parent, key)`, while the cid is derived independently.
