---
"loro-crdt": patch
---

Preserve remote undo transforms when a checkpoint has no container event information, including merged spans. Trim finite undo byte quotas with one history-size pass instead of repeatedly recounting retained items.
