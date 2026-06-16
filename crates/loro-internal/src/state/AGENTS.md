# State Guidelines

This module owns materialized document state, container stores, diff application,
checkout/replay behavior, deep/shallow values, and mergeable container
visibility.

## State Map

- `../state.rs`: `DocState`, checkout/path/deep-value traversal, state replay,
  container lifecycle, and alive-container discovery.
- `container_store/`: persisted KV-backed container snapshots and
  `ContainerWrapper` encoding.
- `map_state.rs`, `list_state.rs`, `richtext_state.rs`, `tree_state.rs`,
  `movable_list_state.rs`, `counter_state.rs`: per-container state and snapshot
  codecs.
- `mergeable.rs`: logical child edge resolution for mergeable containers.
- `dead_containers_cache.rs`: dead/alive tracking, including marker-driven
  mergeable reactivation.
- `unknown_state.rs` and `../diff_calc/unknown.rs`: forward-compatibility support
  for unknown container types.
- `../../docs/mergeable-container-id.md`: mergeable container id format.
- `../../tests/mergeable_container/`: behavior tests for mergeable visibility,
  deletion, conflicts, pending updates, paths/events, and snapshots.

## Mergeable Container Model

Mergeable child containers are created by `MapHandler::ensure_mergeable_*` in
`../handler.rs` and exposed by Rust/WASM wrapper APIs.

Core idea:

- Two peers calling `ensure_mergeable_<kind>(key)` on the same parent map derive
  the same deterministic child `ContainerID` via
  `ContainerID::new_mergeable(parent, key, kind)` in `loro-common`.
- The child id is represented as a reserved `ContainerID::Root` name. The child
  kind lives in `ContainerID::Root.container_type`; the root-name payload only
  encodes parent map identity and map-key path.
- The parent map slot stores a compact binary marker from
  `loro_common::mergeable_marker(parent, key, kind)`. This marker is the source
  of truth for which mergeable child kind is currently visible.
- Parent map LWW semantics resolve concurrent different-kind markers. Losing
  children are hidden but their state must be preserved and can resurface if a
  later `ensure_mergeable_<loser_kind>` rewrites the marker.

Important boundaries:

- User strings, arbitrary binary values, scalars, and regular child containers
  are not mergeable markers and must block `ensure_mergeable_*` rather than be
  overwritten.
- Same-kind `ensure_mergeable_*` over an existing marker is idempotent and should
  not emit another op.
- Different-kind `ensure_mergeable_*` over an existing marker is a deliberate
  kind change and writes a new marker.
- Deleting the map key clears the marker and hides the child, but the child state
  is preserved by deterministic id. Re-ensuring the same kind resurfaces it.
- Visibility comes from the parent marker, not from whether the child already
  has direct ops. This matters for nested mergeable maps and pending imports.

## Mergeable Code Index

- `crates/loro-common/src/lib.rs`: `MERGEABLE_NAMESPACE_PREFIX`,
  `ContainerID::new_mergeable`, `parse_mergeable`, `mergeable_marker`,
  `parse_mergeable_marker`, and marker-to-container translation.
- `../handler.rs`: `MapHandler::ensure_mergeable_container` validates the parent
  slot, writes markers, and returns a handler for the deterministic cid.
- `mergeable.rs`: resolves logical child paths from deterministic cid plus the
  parent map's current marker.
- `map_state.rs`: translates marker values to `LoroValue::Container` at read and
  diff boundaries when the parent id is known.
- `../state.rs`: deep-value/path traversal must recognize marker-backed child
  edges.
- `../txn.rs`: local event diffs translate marker writes into container values
  for subscribers.
- `dead_containers_cache.rs`: import/reactivation behavior when marker values
  change across peers.
- `../../tests/mergeable_cid_encoding.rs`: deterministic cid and parser tests.
- `../../tests/mergeable_container/discriminator.rs`: marker layout,
  idempotency, kind-change, and non-mergeable occupant tests.
- `../../tests/mergeable_container/type_conflict.rs`: concurrent different-kind
  conflict behavior.
- `../../tests/mergeable_container/snapshot.rs`: snapshot and shallow snapshot
  retention, including losing-kind state.
- `../../tests/mergeable_container/pending.rs`: pending updates that arrive
  before all mergeable context exists.
- `../../tests/mergeable_container/events_and_paths.rs`: event and path surface.

## Encoding/Retention Rules

- Snapshot and shallow snapshot alive-container walks must retain mergeable
  child state even when the child is not currently visible because another kind's
  marker wins.
- Raw marker bytes are the wire/storage representation. Public read surfaces
  should translate active markers to container values unless the API explicitly
  exposes raw/shallow storage.
- Mergeable root names grow with nested mergeable map key paths. Avoid adding
  APIs or tests that encourage deep mergeable-map chains without measuring the
  serialized id cost.

## Validation

For mergeable/state changes, start with:

- `cargo test -p loro-internal --test mergeable_cid_encoding`
- `cargo test -p loro-internal --test mergeable_container`
- `cargo test -p loro-internal import_atomicity` if import or rollback is
  involved.

Run root-level broader tests when changing shared replay, checkout, or snapshot
behavior.
