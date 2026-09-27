---
"loro.js": patch
---

Text edits now converge with Rust (`loro-crdt`) in cases a randomized comparison with the Rust WASM build found:

- A concurrent insert after a sibling's subtree was placed after later, unrelated concurrent text. Replicas that merged such edits in different orders could end with different text.
- An imported delete is now applied by its position, as Rust does, instead of by its recorded `start_id`. Rust's WASM build records a wrong `start_id` when the deleted text spans astral characters (for example, emoji) inserted in separate edits, so loro.js deleted different characters than Rust.
- `checkout`, `diff`, and `revertTo` no longer restore a character twice when two concurrent deletes removed it.
- Text cursors follow Rust: a cursor at the end or in empty text has no target, `getCursorPos` returns the target's own offset for every side, and a deleted target reports the length of the text before it with side `-1`. The encoded cursor matches Rust's bytes.
- Consecutive inserts in one transaction are merged into one op, and a delete that spans several ID runs writes the last run first, as Rust does.
