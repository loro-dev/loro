---
"loro-crdt": patch
---

Apply changed hard document quotas to the next transaction while preserving pending edits. Keep commit admission and rollback under one transaction lock, reclaim after soft-only imports, visit every cold container during reclamation, and skip undo byte estimation when no byte quota is set.
