# Tree Parent Validation on Import

Verified against code 2026-11-23 (main `dbe7fc25` plus this change).

A tree `Create`/`Move` names its parent as a node id. The import fast path applies
raw ops with the parent check off (`need_check` is false for `DiffMode::Linear` in
`state/tree_state.rs`), and the event conversion unwraps the parent
(`is_node_deleted(..).unwrap()`). An op naming a node that exists in no history
therefore used to panic with the doc locks held (a WASM trap), or, without a
subscriber, left an orphan in the state. The three diff modes also disagreed on it:
`Linear` stored the orphan, `ImportGreaterUpdates` dropped the op silently,
`Checkout`/`Import` replayed it as effective.

## The rule

A tree `Create`/`Move` whose parent is a node (not the root, not the deleted root),
and a `Delete` of a node, is valid only if a `Create` or `Move` on that node, in the
same tree container, is in the op's causal history (an earlier op of the same change
counts). If history was trimmed by a shallow snapshot, a node that is in the shallow
root state (deleted nodes included) is also valid. Anything else is a
`LoroError::DecodeError` and the import rolls back.

Two lookups implement it, cheapest first:

1. By id (`OpLog::resolve_tree_parent`, `is_visible_tree_parent`): the op that has
   the node's id (`peer@counter` of the `TreeID`), when it is a tree op on that node
   in the same tree, plus one vv check. Every node created by a current build
   resolves here, then the shallow root state.
2. By target (`OpLog::is_visible_tree_parent_by_target`), only when (1) fails: every
   `Create`/`Move` on the node in the tree's import cache
   (`ContainerHistoryCache::get_tree`), checked against the op's vv. Building that
   cache parses the whole history once, so it must stay off the path of valid
   current-format imports.

It reads only the op's own causal history, never the importing doc's state, so a peer
that already has a concurrent change creating the parent decides like a peer that
does not, on every import path.

Why (1) usually suffices: a `TreeID` is the id of the `Create` op that first made
the node. Undo/redo and `revert_to` bring a deleted node back with a `Move` of its
old id (`TreeHandler::revive_for_apply_diff`), and `apply_diff` gives a node the
doc never had a fresh id (`__internal__next_tree_id` before
`create_at_with_target_for_apply_diff`), so the original `Create` is in history.

Why (2) exists: before loro-dev/loro#468 (1.0.0-beta.5), `apply_diff` and undo
re-created a node the doc had never seen under the diff's own `TreeID`, by an op
with another id. The encoding stores such an op as a `Move` (a `Create` is only
encoded when the target equals the op id), so that node has no op with its id in
the history, and (1) alone rejected honest children of it.

## Where

- `OpLog::insert_new_change` records a `TreeParentRef` for each such op into the open
  `ImportRollback` (not for local commits). A parent created earlier in the same change
  by an op that has its id needs no lookup.
- `OpLog::validate_tree_parent_refs_in_import_scope` runs with the other op reference
  checks (`validate_op_refs_in_import_scope`), before any state is applied, so it covers
  the same paths as the movable-list and style-end checks: attached and detached
  imports, `import_batch`, JSON updates, unlocked pending changes, and snapshots
  imported into a non-empty doc (they are imported as changes).
- `import_op_can_reject` opens a scope for detached imports of such ops; attached
  imports already open one for every tree op.

## Known gaps

- A snapshot imported into an empty doc installs its history without validating it op
  by op (see `movable-list-op-validation.md`). The tree state in a snapshot cannot hold
  an orphan (its parents are encoded as indices into the node list), but the history
  of a snapshot written by a build without this check can.
- A `Move` of a target with no op in history is accepted (see (2): it is how old
  builds encoded a re-created node). With a subscriber its event still panics on
  main, independently of this check: the node's metadata container has no known
  parent (`ContainerWrapper::new`, "a container gets state only once its parent is
  known"), since the arena only links metadata containers created by `Create` ops
  (`InnerContent::visit_created_children`) or by the op with the container's id
  (`creator_resolver`). Linking them for such `Move`s is a follow-up.
- A `Move` that closes a cycle is not checked here.

## Tests

- `crates/loro/tests/tree_parent_invalid_ops.rs`.
- `crates/loro-wasm/tests/tree_parent_invalid_ops.test.ts`.
