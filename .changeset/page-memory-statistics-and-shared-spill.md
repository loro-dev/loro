---
"loro-crdt": patch
---

Cache materialized-state memory statistics per 64-slot page and invalidate only pages accessed for mutation. Preserve exact hard-quota checks and compare cached statistics with full recomputation in debug builds. Share immutable spill files across forks without copying their contents, and create Unix spill files with owner-only permissions.
