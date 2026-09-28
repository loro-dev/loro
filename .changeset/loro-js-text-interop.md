---
"loro.js": major
---

Text now follows Rust (`loro-crdt`) everywhere, including when reading documents written by earlier loro.js versions. This breaks documents edited only with loro.js that have concurrent text edits or stored cursors: read "Upgrading from 0.2" in the README before upgrading.

- **Breaking:** concurrent text inserts are ordered like Rust. Earlier versions could place an insert after a sibling's subtree behind later, unrelated concurrent text, so a history with such inserts can now read differently (for example `béa9` instead of `bé9a`). Earlier loro.js replicas that merged such edits in different orders, and documents shared with `loro-crdt` peers, had already diverged; they now agree with Rust.
- **Breaking:** text cursors follow Rust. An encoded cursor stores the Unicode position of its target, not the UTF-16 position, so a cursor encoded by an earlier version can resolve to another offset. A cursor at the end or in empty text has no target, `getCursorPos` returns the target's own offset for every side, and a deleted target reports the length of the text before it with side `-1`. Get new cursors after upgrading.
- An imported delete is applied by its position, as Rust does, instead of by its recorded `start_id` (Rust's WASM build up to 1.16.3 can record a wrong one around astral characters, loro-dev/loro#1149).
- `checkout`, `diff`, and `revertTo` no longer restore a character twice when two concurrent deletes removed it.
- Consecutive inserts in one transaction are merged into one op, and a delete that spans several ID runs writes the last run first, as Rust does.
- Importing a concurrent insert next to a long run of text no longer walks the whole run.

Upgrading: a snapshot written by an earlier version keeps its current text, but its history (updates, JSON updates, checkout of older versions) is read like Rust, and a replica loaded from a snapshot and one loaded from updates can keep different text after new edits. Migrate every replica the same way; the safest path is to read the final state with the old version and build a new document from it.
