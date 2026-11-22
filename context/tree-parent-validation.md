# Tree Parent Validation on Import

Verified against code 2026-11-22 (main `41845694` plus this change).

A tree `Create`/`Move` names its parent as a node id. The import fast path applies
raw ops with the parent check off (`need_check` is false for `DiffMode::Linear` in
`state/tree_state.rs`), and the event conversion unwraps the parent
(`is_node_deleted(..).unwrap()`). An op naming a node that exists in no history
therefore used to panic with the doc locks held (a WASM trap), or, without a
subscriber, left an orphan in the state. The three diff modes also disagreed on it:
`Linear` stored the orphan, `ImportGreaterUpdates` dropped the op silently,
`Checkout`/`Import` replayed it as effective.

## The rule

A tree `Create`/`Move` whose parent is a node (not the root, not the deleted root)
is valid only if the op that has the parent's id (`peer@counter` of the `TreeID`) is
an op on that node, in the same tree container, and is in the op's causal history. If
history was trimmed by a shallow snapshot, a parent that is in the shallow root state
(deleted nodes included) is also valid. Anything else is a `LoroError::DecodeError`
and the import rolls back.

It reads only the op's own causal history, never the importing doc's state, so a peer
that already has a concurrent change creating the parent decides like a peer that
does not, on every import path.

Why "the op with the parent's id": every `TreeID` is the id of the `Create` op that
first made the node. Undo/redo and `apply_diff` revive a node with a new `Create` op
that carries the old id (`TreeHandler::create_at_with_target_for_apply_diff`), but
the original `Create` is still in history, so this lookup finds it. No target index
is needed.

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
- `Delete` or `Move` of a target that never existed, and a `Move` that closes a cycle,
  are not checked here.

## Tests

- `crates/loro/tests/tree_parent_invalid_ops.rs`.
- `crates/loro-wasm/tests/tree_parent_invalid_ops.test.ts`.
