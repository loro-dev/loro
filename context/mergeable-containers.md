# Mergeable Container Context

Verified against code 2026-06-16.

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

## Snapshot And Retention Rules

Snapshot and shallow snapshot alive-container walks must preserve mergeable child
state even when that child is hidden by a different winning marker. This is
covered by `tests/mergeable_container/snapshot.rs`, including shallow snapshot
tests for losing-kind state.

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
- "Kind conflict discards the loser." False; the loser is hidden but should stay
  recoverable by deterministic cid.
- "The marker is the child cid." False; the marker activates a kind at a
  `(parent, key)`, while the cid is derived independently.
