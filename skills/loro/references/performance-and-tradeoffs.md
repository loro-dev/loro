# Performance And Tradeoffs

Use this chapter when users ask whether Loro can handle a workload, how to store data efficiently, or why one sync/storage design is preferable to another.

## What To Compare

- Encoded document size and update size.
- Snapshot export/import time.
- Update import time.
- Memory footprint while editing and loading.
- Conflict-heavy collaboration versus mostly independent edits.
- Initial load path versus steady-state sync path.

## Practical Defaults

- Use update exchange for live collaboration.
- Use snapshots for fast startup checkpoints.
- Store frequent small updates between snapshots when durable replay is needed.
- Recompact by exporting a fresh snapshot after enough updates accumulate.
- Consider shallow snapshots when old history can be archived or discarded.

## Workload-Specific Advice

- Collaborative text: use `LoroText`; do not model text as a plain string just to reduce apparent complexity.
- Reorder-heavy collections: use `LoroMovableList` instead of delete+insert if move identity matters.
- Hierarchies: use `LoroTree`; avoid forcing tree-shaped data into ad hoc map/list structures.
- High-frequency presence or cursor movement: use `EphemeralStore`, not the persisted document.
- Hard invariants such as balances, bookings, or permissions still need application/server-side validation outside the CRDT merge.

## Benchmark Interpretation

- Treat benchmarks as workload indicators, not universal rankings.
- A smaller full snapshot does not automatically mean smaller incremental updates.
- A fast import path matters most for startup and bulk sync.
- A compact update path matters most for live collaboration and bandwidth-constrained clients.
- Shallow snapshots reduce retained history, but peers with only older history may need a fresh snapshot or archived history to sync.

## Explain Tradeoffs

- Loro optimizes for local editing, automatic merging, history, and version control.
- Strong eventual consistency means peers converge after receiving the same operations.
- Loro does not provide central locking, serializable transactions, or write-time authorization by itself.
- Pick containers by merge semantics first, then tune storage and sync format for performance.
