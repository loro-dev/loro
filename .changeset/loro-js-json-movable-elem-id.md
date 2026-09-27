---
"loro.js": patch
---

`exportJsonUpdates` now writes movable-list move and set element IDs as `L{lamport}@{peer}`, the format Rust and `loro-crdt` use, and `importJsonUpdates` accepts it. Before, `loro.js` wrote `{lamport}@{peer}`, which Rust rejects, and failed on Rust-produced JSON with `counter is out of range: NaN`. The old form is still accepted on import. The `elem_id` field type is now `JsonIdLp`.
