---
"loro.js": patch
---

Fix checkout and `diff` on a `loro.js` document imported from a snapshot. Checking out an earlier version could return the latest value of a child container that had not been read yet, restore nothing for elements deleted before the snapshot, skip a delete when moving forward again, or throw `duplicate sequence id`. A transition now rebuilds only the Text and List containers it touches from their own history, once. Checkout events for containers that had not been read are now correct, and a snapshot exported after history was loaded no longer drops containers that had not been read. Rust-created rich text keeps its snapshot styles and text at the snapshot version.
