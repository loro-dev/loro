---
"loro.js": minor
---

Text now follows Rust (`loro-crdt`) everywhere, including when reading documents written by earlier loro.js versions. This breaks documents edited only with loro.js that have concurrent text edits or stored cursors: read "Upgrading from 0.2" in the README before upgrading.

- **Breaking:** concurrent text inserts are ordered like Rust (the fix is shared with loro-dev/loro#1139). Earlier versions could place an insert after a sibling's subtree behind later, unrelated concurrent text, so a history with such inserts can now read differently (for example `béa9` instead of `bé9a`). Earlier loro.js replicas that merged such edits in different orders, and documents shared with `loro-crdt` peers, had already diverged; they now agree with Rust.
- **Breaking:** text cursors follow Rust: a cursor reports its target's own offset, so a cursor encoded by an earlier version with side 1, or at the end of the text, resolves one character earlier (the end cursor of `abc` at 2 instead of 3). Other cursors resolve as before. The encoded origin is now the Unicode position, as in Rust. A cursor at the end or in empty text has no target, `getCursorPos` returns the target's own offset for every side, and a deleted target reports the length of the text before it with side `-1`. Get new cursors after upgrading.
- An imported delete is applied by its position, as Rust does, instead of by its recorded `start_id` (Rust's WASM build up to 1.16.3 can record a wrong one around astral characters, loro-dev/loro#1149).
- `checkout`, `diff`, and `revertTo` no longer restore a character twice when two concurrent deletes removed it.
- Consecutive inserts in one transaction are merged into one op, and a delete that spans several ID runs writes the last run first, as Rust does.

Upgrading: updates and JSON updates written by an earlier version are read like Rust. A snapshot written by an earlier version keeps its current text, but its history reads like Rust, so a document loaded from it must not stay in use (exported updates, replicas loaded from updates, and checkouts can show other content). Exporting a shallow snapshot from such a document switches the document itself to Rust's reading. Migrate every replica the same way: read the final state with the old version and build a new document from it (reading it with 0.3 right after importing an old snapshot only works when that snapshot is the final version, with no updates stored after it), or import the old updates everywhere and accept Rust's reading. See "Upgrading from 0.2" in the README.
