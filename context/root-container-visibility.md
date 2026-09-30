# Root Container Visibility

Verified against code 2026-09-30.

`DocState::get_deep_value` (`crates/loro-internal/src/state.rs`) lists the root
containers that have an entry in the state store (`preferred_root_containers`), so
a root can be visible with an empty value (`{"t": ""}`). `Configure`'s
`hide_empty_root_containers` (off by default) hides those.

## What makes a root visible

- **Any op in the history touches it.** The writing peer creates the root's state
  with its first local op. An import used to create it only when the root's net
  diff was non-empty, so text inserted and then deleted showed `{"t": ""}` on the
  writer and `{}` in a replay of the same history (loro-dev/loro#1156).
  `materialize_touched_roots` (`src/loro.rs`) now creates the state of every
  non-mergeable root that the import's `DiffCalculator` touched
  (`DiffCalculator::touched_containers`). The same happens when a checkout reaches
  the op log's latest version (`attach`, the end of `import_batch`), where every
  range the persistent calculator has seen lies in the target's history. Parked
  pending changes already materialize their roots
  (`pending_root_containers_to_materialize`).
- **Getting a handle.** `doc.get_text("t")` (and the other `get_*` root getters)
  creates the root's state without any op, by design, and snapshots keep it. So
  the value does not depend on the history alone: a peer that only read a root
  shows it, and a replay of its history does not. Changing that is an API
  decision; it is the remaining part of the random workload in #1156.

## Not covered

Checking out an *older* version keeps every root the state already has, including
roots first touched after that version. A fresh replay up to that version would
not show them.

## Tests

`crates/loro/tests/root_visibility_replay.rs`,
`crates/loro-wasm/tests/root_visibility_replay.test.ts`.
