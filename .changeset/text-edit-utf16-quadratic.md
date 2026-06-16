---
"loro-crdt": patch
"loro-crdt-map": patch
---

Fix O(n^2) text editing when using UTF-16 / UTF-8 (byte) positions.

Since 1.12.0, every `insert`/`delete`/`splice`/`mark` that uses UTF-16 or byte
coordinates (the default in the JS binding) validated its position by
materializing the entire `[0, pos)` prefix string and counting its length,
making each edit O(n) and a sequence of edits O(n^2). The boundary check now
reads the rope's prefix caches via the cursor, so it is O(log n) and editing is
linear again. Unicode-indexed editing was unaffected.
