---
"loro.js": patch
---

`loro.js` now records the same local ops as `loro-crdt` for three API calls, so a document edited through either package gets the same op IDs and the same concurrent outcomes:

- `LoroMap.delete(key)` records a delete even when the key is absent. Like in Rust, that delete can win against a concurrent `set` of the same key.
- `LoroCounter.increment(0)` records an increment, and attaching a detached counter records an increment by its value, even 0.
- A `LoroText.delete` that spans several ID runs emits one delete per run from right to left, as Rust does. Checking out a version in the middle of such a delete now shows the same text in both packages.
