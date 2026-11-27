---
"loro-crdt": patch
---

Fix text diffs and checkout events between versions with concurrent marks. A style that the target version removes could come back in the diff, so applying it or mirroring the events gave the wrong formatting.
