---
"loro.js": minor
---

Rich text now uses Rust's position model. A mark's start and end anchors are elements of the text sequence, as in Rust (`loro-crdt`), so every insert, delete, and mark position loro.js writes or reads counts them.

**Breaking:** loro.js reads rich-text history written by earlier versions with Rust's positions. Earlier versions wrote and read positions without the anchors, so every edit made after a mark in a document edited only with loro.js can now apply elsewhere, and styles can cover other text (for example, after marking `ab` in `abcd`, inserting `X` at 3 and deleting 1 gave `acXd` in earlier versions; those ops now read as `bXcd`, as Rust has always read them). Documents shared with `loro-crdt` peers had already diverged there (Rust read `bXcd` from loro.js's updates, loro.js read `acdX` from Rust's); they now agree with Rust. There is no version marker in the data: read "Upgrading from 0.2" in the README before upgrading.

Behavior now follows Rust:

- Text inserted where a mark starts or ends goes inside or outside the mark according to the mark's expand setting, as Rust decides it; concurrent edits at a mark boundary converge with Rust.
- **Breaking:** `mark` and `unmark` throw when `start >= end`, and skip a mark that would change nothing, as Rust does.
- **Breaking:** internal `_`-prefixed members of `LoroText` that were visible in the type declarations (`_insertVisible`, `_applyMark`, `_styleRuns`, `_validateInsertPosition`, `_unicodePosition`, `_detachedStyleCounter`) were removed.
- `applyDelta` drops a style that inserted text would inherit when the insert does not list it, like Rust's `apply_delta`.
- Undo and redo restore the attributes a mark replaced.
- Snapshots write and read the anchors like Rust, so checking out an older version of a styled text gives the same result in both runtimes. Shallow snapshots null the values of styles that no text is left in, like Rust.
- Inserting into a nested text that a snapshot loaded lazily no longer throws or drops the styles at the insert position, and checking out a version that has a mark's start but not its end no longer reports the whole text as inserted.

Upgrading: a snapshot written by an earlier version keeps its current rich text, but its history reads like Rust, so a document loaded from it must not stay in use. Read the final state (`text.toDelta()`) with the old version and build a new document from it (reading it with 0.3 right after importing an old snapshot only works when no updates were stored after that snapshot: with a snapshot of bold `ab` in `abcd` and a later update that inserted `X` at 3 and deleted 1, the old version shows `[a]cXd` and 0.3 and Rust show `[bX]cd`), or import the old updates everywhere and accept Rust's reading. Exporting a shallow snapshot from a document loaded from an old snapshot switches that document itself to Rust's reading.
