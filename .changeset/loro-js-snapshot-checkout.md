---
"loro.js": patch
---

Fix checkout and `diff` on a `loro.js` document imported from a snapshot. Checking out an earlier version could return the latest value of a child container that had not been read yet, restore nothing for elements deleted before the snapshot, skip a delete when moving forward again, or throw `duplicate sequence id`. The first checkout after a snapshot import now replays history once; later checkouts keep using incremental transitions.
