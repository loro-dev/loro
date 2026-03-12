# Sync, Versioning, And Events

## General Sync Guarantees

- If two peers eventually hold the same set of operations, they converge to the same state.
- Delivery order, duplication, and temporary disconnection are transport concerns, not document-consistency failures.
- For steady-state collaboration, sync updates rather than full snapshots whenever possible.

## Sync Patterns

- Give every peer a unique peer ID. Reusing peer IDs can make updates look duplicated and break convergence.
- Prefer update exchange over whole snapshots for steady-state sync:
  - `subscribeLocalUpdates(...)` for push-based transport.
  - `export({ mode: "update", from })` when you track a known sync point.
- Use `importBatch(...)` for multiple updates or mixed snapshot/update payloads. It performs one diff calculation and emits one combined event.
- `EphemeralStore` local updates should be transported separately from persisted document updates.

## Version Representations

- `VersionVector`: explicit peer-to-counter map. Best for synchronization, inclusion checks, and self-contained network state.
- `Frontiers`: compact boundary operations. Best for checkpoints and time travel.
- Convert between them when needed; vectors are explicit, frontiers are compact.

## State Vs History

- `version()` / `frontiers()`: the current visible DocState.
- `oplogVersion()` / `oplogFrontiers()`: the latest history known to the OpLog.
- When attached, state and history usually align.
- After `checkout(...)`, DocState can diverge from OpLog. That is detached mode.

## Attached Vs Detached

- Detached document state means “viewing history” or otherwise not showing the latest OpLog.
- Detached container state means “not yet inserted into a document.”
- These are separate concepts.

## `checkout(...)` Versus `revertTo(...)`

- `checkout(frontiers)`: move document state to an old version. This puts the doc into detached mode.
- `checkoutToLatest()` or `attach()`: return to the latest attached state.
- `revertTo(frontiers)`: generate new operations that transform the current document into the target historical state. The doc stays attached.

## Peer IDs

- Every operation ID is `(peerId, counter)`.
- Never reuse peer IDs across concurrent sessions.
- Do not use user IDs as peer IDs.

## Operations, Changes, Transactions

- Operations are atomic edits.
- Changes are logical groups of operations with metadata.
- Transactions batch operations for event emission and grouping.
- Loro transactions are not ACID transactions and do not provide rollback semantics.
- Changes can carry timestamp, commit message, dependencies, peer identity, and other metadata.
- Consecutive operations may merge into one Change depending on merge rules and timing configuration.

## Import Status

- `ImportStatus.success` tells you what applied.
- `ImportStatus.pending` tells you what is blocked on missing dependencies.
- Check pending ranges in out-of-order networks.

## Event Timing Caveat

- Some older examples describe JS events as microtask-delayed.
- Modern `loro-crdt` dispatches events synchronously at the JS boundary.
- Always check the package version and current codebase before assuming old timing semantics.

## Event Sources

- Events can be triggered by local commits, imports, and checkouts.
- Implicit commits happen around operations like export, import, and checkout.

## `subscribePreCommit(...)`

- Use it when commit metadata or change-level hooks must catch both explicit and implicit commits.
- It is the right place for setting commit message and timestamp.
- Combined with `exportJsonInIdSpan(...)`, it can drive change hashing or Merkle-DAG-like workflows.

## Timestamps

- Timestamp recording is configurable.
- `setRecordTimestamp(...)` and `setChangeMergeInterval(...)` affect how changes merge over time.
- These settings are runtime configuration, not durable document state.

## Inspector

- Use Loro Inspector when you need to browse current state plus full edit history interactively.
- Prefer it for “what happened to this doc?” debugging before writing custom debug UI.

## Sync Debugging

- If imports arrive out of order, inspect `ImportStatus.pending`.
- If checkout seems stale, compare DocState version to OpLog version.
- If a hook missed an implicit commit, audit `subscribePreCommit(...)` rather than adding ad hoc commit calls.

## Undo, Cursor, And Ephemeral Presence

- `UndoManager` is local to one peer. When peer identity changes, the undo and redo stacks reset.
- Use `onPush` and `onPop` to store selection state together with undo items.
- Use `groupStart()` and `groupEnd()` when one UX action spans multiple commits.
- Use `Cursor`, not absolute indices, for selections and carets that must survive concurrent edits.
- Store two cursors for a selection: anchor and head.
- Persist the updated cursor returned by `doc.getCursorPos(cursor)` when available; it reduces future replay cost.
- Use `EphemeralStore` for presence, cursor payloads, hover state, and other collaboration data that must not persist in the CRDT document.
