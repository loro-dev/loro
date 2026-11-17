---
"loro-crdt": minor
---

Add per-document memory budgets, memory statistics, explicit reclamation and `tryCommit()`, plus UndoManager byte limits. Bound caches and demote cold state while preserving exact hard-quota checks and atomic rollback of rejected imports. Reduce retained memory with compact internal layouts and page-cached state statistics. Native Rust supports file-backed history and state spilling, shared immutable spill files, checked reads and reportable storage errors; forks inherit independent memory policies.
