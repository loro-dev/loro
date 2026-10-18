# Deep tree traversal

Verified against code 2026-10-18.

`crates/loro-internal/src/state/tree_state.rs` stores tree nodes and parent
links in flat maps. Tree depth must not consume one call frame per level
(loro-dev/loro#1195). Snapshot import is lazy: the first `roots()` or `nodes()`
read reaches `TreeState::get_value` via `ContainerWrapper::decode_value_from_bytes`,
even when the caller only requests a flat result.

- `get_all_hierarchy_nodes_under` builds nested results with an explicit work
  stack. `TreeNodeWithChildren::into_value` finishes children before parents;
  `get_meta_value` resolves metadata with an explicit stack.
- `is_ancestor_of` and `is_node_deleted` follow parent links in loops. A walk
  longer than the number of stored nodes is an internal cycle and fails fast;
  it must not hang or return a fabricated ancestor/deletion result.
- `_bfs_all_nodes` retains the historical snapshot order despite its name:
  emit a whole sibling group, then visit each sibling's subtree in order.
  Replacing it with a conventional breadth-first walk changes encoded order.
- Attached tree copying, unknown-container preflight, and WASM hierarchy
  construction also walk tree children iteratively.

Iteration during construction alone is insufficient. The temporary nested
`LoroValue` made on first decode is discarded, and the default map/list `Arc`
destructors would still overflow. `crates/loro-common/src/value.rs` releases
owned descendants iteratively. Shared descendants stay alive for their other
owners. Its shared-owner path uses `Arc::into_inner`, whose concurrent last-owner
guarantee is required; `Arc::try_unwrap(...).ok()` can instead trigger recursive
payload destruction. `TreeNodeWithChildren` similarly drains children on drop.
WASM's outbound `LoroValue` conversion must also avoid per-level recursion.

The shared value destruction, `unwrap()` and outbound conversion are inherited
unchanged from PR #1204 (`6ae57344`), which includes main `089a4e64`. Its
shared-owner path reuses static empty Arcs; see
[deep-value-stack-safety.md](deep-value-stack-safety.md). Main #1197 added
`LoroListValue::into_arc()`; #1204 takes its Arc with `mem::take` so it remains
compatible with Drop. The tree commit adds no second Drop implementation and
does not change `value.rs`. The iterative rich-text mark snapshot codec, shallow
style redaction, style hashing and deep-value export also remain unchanged from
#1204, as does the 128-level limit for external JSON/serde deserialization.

This does not impose a maximum tree depth, change the wire format or API result
shape, or make arbitrary user formatting/serialization of a nested value
stack-independent. Nested metadata containers are a separate container-depth
problem.

Tests: `crates/loro/tests/tree_deep_chain.rs` runs the issue's exact 4,000-level
snapshot/1 MiB read reproduction in a subprocess, plus values, export, parent
walks, copying and concurrent result release. `tree_state::traversal_tests`
covers hierarchy disposal and sibling/subtree order; `value::stack_safety_tests`
from #1204 covers shared-owner destruction and unwrap behavior.
`crates/loro-wasm/tests/tree_deep_chain.test.ts`
checks the corresponding WASM reads in a separate process.
