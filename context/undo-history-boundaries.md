# Undo/redo history boundaries

Verified against code 2026-11-12.

`crates/loro-internal/src/undo.rs`: `UndoManager::perform` processes at most
one recorded `StackItem` per call to `undo` or `redo`. Grouping and the merge
interval can put several edits, including edits to different containers, into
that one item.

The merge window starts when a new item is actually added, not merely when
`record_checkpoint` decides against requesting a merge. `Stack::push_with_merge`
can reject a requested merge after a remote import, on an empty tail row, or
when counter spans are not contiguous. These new items reset `last_undo_time`;
actual merges keep the item's original timestamp (#1233). The timer uses wall
clock milliseconds, independently of commit timestamps. Deterministic coverage
lives in `undo.rs::merge_timer_tests` and `crates/loro-wasm/tests/undo.test.ts`.
The WASM `setOnPush` span check still identifies the newly created item and keeps
its metadata when later pushes merge into it (#1253).

Remote edits can make the item's inverse a no-op. For example, inserting a
MovableList element locally and then deleting it remotely leaves nothing for
the inverse delete to remove. A move/set whose element was deleted remotely
also cannot write an inverse. That item's lack of generated operations must
not cause `perform` to pop another item: the next item may create an unrelated
parent container, which would then disappear in the same undo call (#1183).

A consumed no-op still runs the existing `on_pop` and cursor bookkeeping. It
returns `false`, generates no opposite-stack entry, and leaves other items for
later calls. Callers must check `can_undo` / `can_redo` to decide whether history
remains. The public Rust and WASM wrappers share this implementation.

`loro-js/src/runtime/undo.ts`: `UndoManager.undo` and `redo` also consume one
item. Their result is whether `#invert` produced an opposite-stack item, not
whether an item was popped. An empty inverse returns `false` while preserving
pop callbacks, tracked-range cleanup and the remaining history.

`LoroDoc._subscribeUndo` in `loro-js/src/runtime/document.ts` also delivers
local commits with empty state diffs to the manager, while public state
subscriptions remain silent. Without this notification, a local MovableList
insertion and deletion grouped into one item is never recorded, and undo
instead consumes an earlier item. This is the `insert/delete undo (one peer)`
differential profile, seed 2, step 33; changing the boolean return alone does
not fix it. The subscription cleanup removes both listeners.

This boundary rule does not change grouping or error handling. MovableList
identity replay (#1209) deletes an inserted element even if it moved remotely,
so remote movement alone is no longer a no-op trigger. The regression tests
use remote deletion instead, including a missing move target for redo.

Regressions:

- `crates/loro/tests/integration_test/undo_test.rs`: `noop_history_boundaries`
  covers undo before a scalar edit or parent creation, redo before a parent
  creation, unchanged versions for no-ops, subsequent history and convergence.
- `crates/loro-internal/tests/undo.rs`:
  `consecutive_noops_consume_one_item_and_notify_once` covers multiple no-ops
  and one pop callback per item.
- `crates/loro-wasm/tests/undo.test.ts` covers the exposed JS methods.
- `loro-js/tests/undo-noop.test.ts` covers no-op undo and redo return values,
  unchanged versions/frontiers, pop callbacks, subsequent parent history and
  notification cleanup when the manager is destroyed.

## Undo stack limit

`UndoManagerInner::trim_undo_stack` removes the oldest undo items after a local
checkpoint, after redo pushes an undo item, and immediately when
`set_max_undo_steps` changes the limit (#1232). Zero drops all undo items and
prevents new ones from being retained; redo history is preserved. Eviction drops
the item's metadata with it and does not invoke `on_pop`, which is reserved for
consuming history through undo/redo. The WASM wrapper stores metadata on these
same Rust items by counter span, without a parallel JS metadata stack.

`crates/loro-internal/tests/undo.rs` and `crates/loro-wasm/tests/undo.test.ts`
cover dynamic shrinking, redo at the limit, zero, and metadata/cursor callbacks.

## Split spans within one history item

`src/undo.rs::undo` processes the peer's spans oldest first. For span `i`,
`A_i` checks out from its last op to its dependencies. The accumulated inverse
of earlier spans is transformed only as far as these dependencies, so it can
be composed directly after `A_i`. Taking it through span `i` and back through
`A_i` loses information: backspaces restore text in reverse order (#1245),
and a replacement can revive intermediate text (#1228). Only the final span
transforms its accumulated inverse to the latest state, using either the
frontiers or the undo manager's supplied remote diff. Undo and redo share this
calculation.

Inverse map writes compose in execution order (`DiffBatch::compose_inverse`),
even when the next inverse restores an older timestamp. Ordinary map event
composition still resolves by timestamp. MovableList native plans still capture
each span's before/after state and undo them newest first; changing the forward
bridge does not change their identity or anchor rules.

`crates/loro/tests/undo_split_spans.rs` covers two/three/five backspaces,
intermediate-state dependencies imported before/after group end, redo cycles,
2,000 seeded random groups with remote edits, concurrent mixed-container edits,
and MovableList child revival. Imports touching a grouped container end the
manual group, so overlapping remote edits are delivered after group end.
`src/undo/split_span_tests.rs` checks the final bridge to latest and a large cut
whose `fork()` has more than two history spans at decoded block boundaries.
