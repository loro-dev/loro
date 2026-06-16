---
"loro-crdt": patch
"loro-crdt-map": patch
---

Fix two O(n^2) editing slowdowns.

1. Editing with UTF-16 / UTF-8 (byte) positions (the default in the JS binding)
   validated each position by materializing the entire `[0, pos)` prefix string,
   making every `insert`/`delete`/`splice`/`mark` O(n) and a run of edits O(n^2)
   (regression since 1.12.0). The boundary check now reads the rope's prefix
   caches via the cursor (O(log n)). Unicode-indexed editing was unaffected.

2. When a subscriber is attached and many edits land on the same container within
   one event batch (e.g. random-position inserts, or many distinct map-key
   writes), building the event cloned the growing accumulated diff on every
   compose — O(n^2) in the number of fragments. The diffs are now composed in
   place. This affected text, map and list events.
