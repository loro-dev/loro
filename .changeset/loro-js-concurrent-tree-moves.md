---
"loro.js": patch
---

Match Rust when concurrent tree moves would create a cycle: skip the move that
closes the cycle and keep all nodes reachable. Preserve the effective placements
through later moves, update replay, checkout, snapshots, events, and undo.
