# State Guidelines

This module owns materialized document state, container stores, diff application,
checkout/replay behavior, deep/shallow values, and mergeable container
visibility. Read
[../../../../context/mergeable-containers.md](../../../../context/mergeable-containers.md)
before changing mergeable child behavior.

## Local Entry Points

- `../state.rs`: `DocState`, checkout/path/deep-value traversal, state replay,
  lifecycle, and alive-container discovery.
- `container_store/`: persisted KV-backed container snapshots and
  `ContainerWrapper` encoding.
- `map_state.rs`, `list_state.rs`, `richtext_state.rs`, `tree_state.rs`,
  `movable_list_state.rs`, `counter_state.rs`: per-container state and snapshot
  codecs.
- `mergeable.rs`: logical child edge resolution for mergeable containers.
- `dead_containers_cache.rs`: dead/alive tracking and marker-driven mergeable
  reactivation.
- `unknown_state.rs` and `../diff_calc/unknown.rs`: forward compatibility for
  unknown container types.

## Mergeable Rules

- `MapHandler::ensure_mergeable_*` writes a compact marker into the parent map
  and returns a handler for a deterministic `ContainerID`.
- The parent map marker, not "child has ops", decides whether a mergeable child
  is visible.
- Non-mergeable occupants must block `ensure_mergeable_*`; same-kind marker
  writes are idempotent; different-kind marker writes are deliberate kind
  changes.
- Snapshot and shallow snapshot retention must preserve hidden losing-kind
  mergeable state.

## Validation

- `cargo test -p loro-internal --test mergeable_cid_encoding`
- `cargo test -p loro-internal --test mergeable_container`
- `cargo test -p loro-internal import_atomicity` if import or rollback is involved.
