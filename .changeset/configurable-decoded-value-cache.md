---
"loro-crdt": minor
---

Add `LoroDoc.setDecodedValueCacheLimit()` and `getDecodedValueCacheLimit()` so applications can choose a finite decoded container value cache limit for repeated handler reads of large snapshots. Keep the default at 2048. Lowering the limit evicts cached values safely; zero disables this cache. Forks inherit an independent copy of the local setting, which is not serialized.
