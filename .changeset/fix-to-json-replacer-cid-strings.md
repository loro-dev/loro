---
"loro-crdt": patch
---

Keep malformed `cid:`-prefixed strings returned from `toJsonWithReplacer` as
plain strings instead of throwing while resolving them as container IDs.
