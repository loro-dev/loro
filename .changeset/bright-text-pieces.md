---
"loro.js": minor
---

Store text operations in shared range-backed buffers, read visible spans without
allocating scalar views, add lazy line navigation plus explicit text compaction,
coalesce adjacent pending text inserts, avoid redundant history indexing during
large single commits, and speed up per-commit local-update encoding with numeric
varints, exact-size buffers, and canonical single-change fast paths.
