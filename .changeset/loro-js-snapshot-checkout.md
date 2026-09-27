---
"loro.js": patch
---

Fix checkout and `diff` on a `loro.js` document imported from a snapshot. Checking out an earlier version could return the latest value of a child container that had not been read yet, restore nothing for elements deleted before the snapshot, skip a delete when moving forward again, or throw `duplicate sequence id`. A transition now rebuilds only the Text and List containers it touches from their own history, once. Rust-created rich text whose history `loro.js` cannot replay keeps its snapshot state, so its latest state and exports stay exact. Checkout events for containers that had not been read are now correct, and a snapshot exported after history was loaded no longer drops containers that had not been read. A character deleted concurrently by two peers is no longer deleted twice during checkout, and a checkout that throws leaves the document unchanged.
