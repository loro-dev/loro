---
"loro-js": minor
---

Improve pure TypeScript runtime performance for merged changes, concurrent
sequence insertion, large state snapshots, and bulk list edits. Snapshot
SSTables now use interoperable LZ4 compression and defer non-frontier history
decoding until a history-dependent API needs it.
