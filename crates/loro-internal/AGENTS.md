# loro-internal Guidelines

This crate contains Loro's unstable internal CRDT implementation. Public API
compatibility concerns still matter because `crates/loro` and `crates/loro-wasm`
wrap this crate directly, but the internal priority is preserving invariants
over graceful degradation.

## Internal Map

- `src/loro.rs`: document-level orchestration for commit, import/export,
  checkout, barriers, state/oplog coordination, and event emission.
- `src/encoding.rs`: public/internal `ExportMode`, binary header parsing,
  checksum verification, `EncodeMode` dispatch, import metadata, and the bridge
  from decoded changes into `OpLog`.
- `src/encoding/`: concrete binary and JSON encoding implementations. Read
  `src/encoding/AGENTS.md` and
  [../../context/internal-encoding.md](../../context/internal-encoding.md)
  before changing binary layout, JSON schema, import metadata, shallow snapshot,
  or op/value encoding.
- `src/oplog/` and `src/dag/`: change storage, dependency ordering, pending
  changes, version vectors/frontiers, shallow roots, and history traversal.
  `ChangeStore` is also read without the op log lock, so keep its lock order:
  [../../context/arena-parent-links.md](../../context/arena-parent-links.md).
- `src/arena.rs`: container IDs, indices, and parent links. How a parent is
  found in a lazily loaded document, and the locking rules:
  [../../context/arena-parent-links.md](../../context/arena-parent-links.md).
- `src/state.rs` and `src/state/`: materialized document state, container stores,
  diff application, checkout/replay, deep value, dead-container tracking, and
  mergeable container visibility. Read `src/state/AGENTS.md` and
  [../../context/mergeable-containers.md](../../context/mergeable-containers.md)
  before changing mergeable containers.
- `src/handler.rs`: typed container handlers, local operation creation, and
  `MapHandler::ensure_mergeable_*`.
  `src/handler/movable_list_apply_delta.rs` applies movable-list diffs for
  `apply_diff` and undo:
  [../../context/movable-list-apply-diff.md](../../context/movable-list-apply-diff.md).
- `src/diff_calc/`: diff calculation when moving between versions. The tree
  calculator's cache transitions:
  [../../context/tree-checkout-window.md](../../context/tree-checkout-window.md).
  Which tree diff modes carry raw ops, and how tree events are built for each:
  [../../context/tree-events.md](../../context/tree-events.md).
- `src/container/richtext/`: text state with style anchors. Where local inserts
  go next to anchors, the insert cursor cache, and delete `start_id`s:
  [../../context/richtext-insert-positions.md](../../context/richtext-insert-positions.md).
- `docs/diff_calc.md`: design notes for diff calculation.
- `docs/critical-version-spec.md`: specification and proof skeleton for
  replay-base selection (Eg-walker-aligned terminology; defines critical
  version, the entry check, and the fallback sweep).
- `docs/mergeable-container-id.md`: current mergeable container id encoding.
- `tests/mergeable_container/` and `tests/mergeable_cid_encoding.rs`: focused
  mergeable container regression tests.
- `src/tests/import_atomicity.rs`: import rollback and malformed-input
  regressions. A failed import keeps its arena indices and drops only parent
  links, values, and the dead-container cache:
  [../../context/failed-import-arena-indices.md](../../context/failed-import-arena-indices.md).
- Movable-list `Move`/`Set` element validation on import, plus change-store
  rollback records and KV block-range decoding:
  [../../context/movable-list-op-validation.md](../../context/movable-list-op-validation.md).
- Root container visibility after import/checkout (`materialize_touched_roots`):
  [../../context/root-container-visibility.md](../../context/root-container-visibility.md).
- Imported changes that reuse local op ids are checked against local history
  before anything is applied (`src/oplog/known_history.rs`):
  [../../context/import-peer-id-reuse.md](../../context/import-peer-id-reuse.md).
- `import_batch` force-detach, its batch-wide rollback scope, and the
  never-exit-detached invariant:
  [../../context/import-batch-atomicity.md](../../context/import-batch-atomicity.md).

## Commands

Use narrow checks first:

- `cargo check -p loro-internal`
- `cargo test -p loro-internal --doc`
- `cargo test -p loro-internal --test mergeable_container`
- `cargo test -p loro-internal --test mergeable_cid_encoding`
- `cargo test -p loro-internal import_atomicity`

For broad shared behavior, run the root commands from `AGENTS.md`. For changes
to import, checkout, encoding, state replay, or diff calculation, consider fuzz
coverage under `crates/fuzz` and ask before running long fuzz targets.

## Working Rules

- Replay-base selection uses Eg-walker terminology (arXiv:2409.14252 §3.5):
  a version V is **critical** when every event outside `Events(V)` happened
  after all of `Events(V)` — no concurrency crosses the cut. Non-`Checkout`
  diff modes are only sound when the base satisfies this (the tree
  calculator's `Checkout` path takes its window from the two versions
  instead); `dag.rs`/`oplog.rs` enforce it via the
  `ImportGreaterUpdates` entry check and, on conservative retreat, the
  multi-head fixpoint `OpLog::latest_critical_version_below_meet`, with
  the `latest_single_head_critical_version` descent as fallback. Do not
  use "LCA" or "common ancestor" in new code or docs: the meet of two
  versions is generally NOT a safe replay base, and a caller that needs a
  critical version must ask for one (loro-dev/loro#1095). Read
  `docs/critical-version-spec.md` before touching `find_replay_base` or diff
  modes.
  `OpLog::iter_from_replay_base_causally` may bypass the DAG when the
  concurrency is register-only (`docs/diff_calc.md`, "Register-only
  concurrency"); keep that check container-granular and history-based.
- Internal invariant violation should fail fast. Invalid external bytes or JSON
  should return `Err`.
- Do not silently skip ops, containers, state entries, diffs, or pending changes.
- Snapshot/import paths must be atomic: if decode or state application fails,
  rollback must leave the document usable.
- Preserve attached/detached document state when export paths temporarily
  checkout another version.
- If a change affects `crates/loro` or `crates/loro-wasm` behavior, add or update
  tests at the wrapper layer as well as the internal layer when practical.
