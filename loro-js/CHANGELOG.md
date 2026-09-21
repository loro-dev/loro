# loro.js

## 0.2.1

### Patch Changes

- be0e1ac: Fix tree snapshots after `move()` or `delete()` (loro-dev/loro#1088). The tree
  state encoder now writes each parent's children in (fractional index, lamport,
  peer) order, uses the last move/delete operation id, and encodes deleted nodes
  with the default fractional index, matching Rust's tree state layout.
  `snapshot` and `shallow-snapshot` exports from documents whose trees had moved
  nodes previously imported into `loro-crdt` without error, then panicked on the
  first read.

## 0.2.0

### Minor Changes

- 880028d: Store text operations in shared range-backed buffers, read visible spans without
  allocating scalar views, and add lazy line navigation plus explicit text compaction.
- 5ea2e37: Add `pause()`, `resume()`, and `isPaused()` to `UndoManager`. While paused,
  local edits are not recorded as undo steps and checkout events do not clear the
  stacks. Import events (remote changes) are still processed so that the stacks
  remain correctly transformed against concurrent edits. Use this to preserve
  undo/redo history across temporary checkouts such as read-only history previews.

## 0.1.0

### Minor Changes

- 68587bc: Add a pure TypeScript implementation of the current Loro binary format and a
  `loro-crdt`-compatible CRDT runtime.
- 68587bc: Improve pure TypeScript runtime performance for merged changes, concurrent
  sequence insertion, large state snapshots, and bulk list edits. Snapshot
  SSTables now use interoperable LZ4 compression and defer non-frontier history
  decoding until a history-dependent API needs it.

### Patch Changes

- 4e663a1: Keep large latest-state snapshots encoded and hydrate containers on demand.
  Local edits and later updates now use a small history overlay, while snapshot
  export rewrites only dirty SSTable blocks and avoids redundant output buffers.
