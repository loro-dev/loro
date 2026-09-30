# Tree Events

Verified against code 2026-09-30.

Tree events (`TreeExternalDiff::{Create, Move, Delete}`) describe only the
alive tree, the nodes reachable from the root. Applied in order, the events of
one batch must turn the alive tree before it into the alive tree after it:

- `Create` for every node that becomes alive, including each node below a
  node that leaves a deleted subtree (the subtree comes back with it);
- `Delete` only for the root of a subtree that stops being alive: it stands
  for the whole subtree, with no `Delete` for the descendants;
- no event for changes inside deleted subtrees;
- every `index` / `old_index` is valid when the events are applied in order.

`trigger_on_new_container` (`crates/loro-internal/src/state.rs`) sends the
metadata map of each `Create` target as a new container, so a revived node's
metadata arrives in map events right after the tree events.

## Two ways to build the events

`TreeState::apply_diff_and_convert`
(`crates/loro-internal/src/state/tree_state.rs`) gets a diff whose shape
depends on the mode returned by `TreeDiffCalculator::calculate_diff`
(`crates/loro-internal/src/diff_calc/tree.rs`):

- `DiffMode::Checkout` (checkouts and concurrent imports): the calculator
  classifies each item against its own cache (`TreeDeltaItem::new`:
  `Create`, `Move`, `Delete`, `MoveInDelete`, `UnCreate`) and follows every
  `Create` with its current subtree (`TreeCacheForDiff::push_children_creation`).
  The state only converts items.
- `DiffMode::Linear` and `DiffMode::ImportGreaterUpdates`: the items are the
  raw ops, sorted by lamport. `TreeInternalDiff::Create` / `Move` only name
  the op type, not what happens to the alive tree. The state classifies each
  op by whether the target is alive before and after it
  (`TreeState::apply_raw_move_and_convert`) and, when a node that existed
  becomes alive, pushes a `Create` for its subtree from the state's children
  lists (`TreeState::push_subtree_creation`). A `Create` or `Move` that leaves
  the node alive where it was produces no event.

Before 2026-09-30 the raw path treated a `Move` that revived a node as a
single `Create` and a `Create` op as a `Create` whenever its parent was
alive, so (loro-dev/loro#1157):

- the nodes below a node moved out of a deleted subtree got no `Create`;
- in `Linear` mode a move of an alive node under a deleted one emitted
  nothing instead of `Delete`;
- in `ImportGreaterUpdates` mode a `Create` op skipped the cycle check that
  `apply_diff` applies, so the state could differ with and without a
  subscriber.

The raw path only runs when the op log has no concurrency with the current
state (`Linear`), or when the new ops all follow the state's version
(`ImportGreaterUpdates`), for example a peer that only receives updates. Two-
or three-peer repros that sync both ways usually take the `Checkout` path.

## Local events are different

Local tree ops build their events from `EventHint::Tree` in
`crates/loro-internal/src/handler/tree.rs` and do not go through
`apply_diff_and_convert`. The local API allows creating under, moving into and
moving out of deleted nodes, and those hints still use the shapes of alive
nodes (`Create` under a deleted parent, `Move` with a deleted `old_parent`).
The undo manager inverts those events, so changing them is a separate change.

## Tests

- `crates/loro/tests/tree_revival_events.rs`: a mirror rebuilt only from
  events (children lists and metadata) is compared with the state after
  imports: the repro blobs from the issue, `Linear` and
  `ImportGreaterUpdates` revivals, a move into a deleted subtree, an undone
  delete, and `random_sync_events_rebuild_the_tree` (random tree edits on
  three peers plus two observers that only import; wider sweeps with
  `TREE_EVENT_SEEDS=0..3000 cargo test --release -p loro --test tree_revival_events`).
  Local edits reset the mirror (see above).
- `crates/loro-wasm/tests/tree_revival_events.test.ts`: the two `Linear`
  cases through `loro-crdt`.
- The fuzz `Actor` tracker (`crates/fuzz/src/actor.rs`) does not catch wrong
  events on its own: `assert_value_eq` only logs a mismatch, so only a panic
  inside the tracker fails a run.
