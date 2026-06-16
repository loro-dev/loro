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

3. Converting a UTF-16 / UTF-8 position within a text chunk to a unicode offset
   scanned the chunk char-by-char, so editing/slicing a large contiguous chunk
   (a big insert, a loaded document, or a long run of typed text that merges into
   one chunk) was O(chunk length) per op. Chunks that contain no astral-plane
   characters (UTF-16) or are pure ASCII (UTF-8) now convert in O(1), covering
   essentially all real-world text (ASCII/Latin/CJK).
