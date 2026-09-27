---
"loro.js": minor
---

Rich text now uses Rust's position model. A mark's start and end anchors are elements of the text sequence, as in Rust (`loro-crdt`), so every insert, delete, and mark position loro.js writes or reads counts them. Before, loro.js wrote and read positions without the anchors, so as soon as a document had a mark, loro.js and Rust applied each other's later edits at different places (for example, after marking `ab` in `abcd`, inserting `X` at 3 and deleting 1 gave `acXd` in Rust but `acdX` when loro.js imported Rust's update, and `bXcd` when Rust imported loro.js's).

Behavior now follows Rust:

- Text inserted where a mark starts or ends goes inside or outside the mark according to the mark's expand setting, as Rust decides it; concurrent edits at a mark boundary converge with Rust.
- `mark` and `unmark` skip a mark that would change nothing, and throw when `start >= end`.
- `applyDelta` drops a style that inserted text would inherit when the insert does not list it, like Rust's `apply_delta`.
- Undo and redo restore the attributes a mark replaced.
- Snapshots write and read the anchors like Rust, so checking out an older version of a styled text gives the same result in both runtimes.

Documents that loro.js edited after a mark with an earlier version contain positions that only loro.js read that way; Rust has always read them with the anchors, and loro.js now does too.
