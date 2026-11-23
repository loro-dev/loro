---
"loro-crdt": patch
---

Replay concurrent text branches with larger peer IDs first, including branches that become ready during replay, to avoid peer-ID-dependent quadratic import work.
