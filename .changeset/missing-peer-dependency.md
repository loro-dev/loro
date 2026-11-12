---
"loro-crdt": patch
---

Reject imported changes whose dependencies do not causally include their same-peer predecessor, preventing malformed updates from poisoning later sync exports. Preserve valid transitive dependencies and out-of-order delivery, and recheck affected pending changes before they are unlocked.
