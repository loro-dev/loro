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
  `ContainerWrapper` encoding. The decoded-value cache in `InnerStore` is
  bounded and evicted wrappers must stay re-creatable from KV; read
  [../../../../context/container-value-cache.md](../../../../context/container-value-cache.md)
  before changing read/caching paths there.
- `map_state.rs`, `list_state.rs`, `richtext_state.rs`, `tree_state.rs`,
  `movable_list_state.rs`, `counter_state.rs`: per-container state and snapshot
  codecs. `richtext_state.rs` also hosts `redact_dead_style_values`, used by
  shallow-snapshot export; read
  [../../../../context/shallow-snapshot-style-redaction.md](../../../../context/shallow-snapshot-style-redaction.md)
  before changing the richtext snapshot codec or that pass.
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

`read_state.rs` traverses ephemeral shallow values into a sink. Container identity
comes from CRDT edges, including mergeable markers and Tree metadata; ordinary
values remain opaque. See [bulk reads](../../../../context/wasm-bulk-read.md).
