---
"loro-crdt": patch
---

Fix three rich-text bugs that a randomized comparison with loro.js found:

- Text inserted next to a style anchor could land on the wrong side of it, depending on an internal cursor cache (native Rust and WASM): for example, after an `unmark` that changed nothing, typing right after a mark that does not expand after put the new text inside the mark.
- In the WASM build, a delete spanning text from separate inserts could record a `start_id` naming the wrong characters when an astral character (for example, an emoji) ended one of the inserts. Imports apply deletes by position, but shallow snapshot imports and cursors on deleted text use the recorded IDs, so they could disagree with a full import. This fixes new ops only; deletes already in a history keep their IDs (loro-dev/loro#1149).
- In the WASM build, committing a transaction that merged backspaces over astral characters panicked with "Op/hint length mismatch" when the document had a subscriber or an `UndoManager`.
