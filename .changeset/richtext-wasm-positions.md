---
"loro-crdt": patch
---

Fix three rich-text bugs in the WASM build that a randomized comparison with loro.js found:

- Text inserted right after a style anchor could land on the wrong side of it, depending on an internal cursor cache: for example, typing after a mark that does not expand after, following an unmark that changed nothing, put the new text inside the mark.
- A delete that spans astral characters (for example, emoji) from separate inserts recorded a `start_id` that named the wrong characters. Rust applies deletes by position, so its own state was right, but other readers of the op were not.
- A transaction that merged backspaces over astral characters panicked with "Op/hint length mismatch" when the document had a subscriber.
