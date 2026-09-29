---
"loro.js": patch
---

`loro.js` now records the same local ops as `loro-crdt` for these API calls, so a document edited through either package gets the same op IDs and the same concurrent outcomes:

- `LoroMap.delete(key)` records a delete even when the key is absent. Like in Rust, that delete can win against a concurrent `set` of the same key.
- `LoroCounter.increment(0)` records an increment, and attaching a detached counter records an increment by its value, even 0.
