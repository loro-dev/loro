---
"loro-crdt": patch
---

Fix shallow snapshots that exported successfully but could not be imported into an empty document. The shallow root is now always a critical version of the retained history. Previously two shapes picked a root that other retained changes were concurrent with: a version with an odd number of independent heads, and a past version when a branch that forked below it was merged later.
