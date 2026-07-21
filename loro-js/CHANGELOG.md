# loro.js

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
