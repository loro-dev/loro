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

Upgrading: updates and JSON updates written by an earlier version are read like Rust. A snapshot written by an earlier version keeps its current text, but its history reads like Rust, so a document loaded from it must not stay in use (exported updates, replicas loaded from updates, and checkouts can show other content). Migrate every replica the same way: read the final state (with the old version, or right after importing an old snapshot) and build a new document from it, or import the old updates everywhere and accept Rust's reading. See "Upgrading from 0.2" in the README.
