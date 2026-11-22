---
"loro-crdt": patch
---

Reject imported updates with a tree create or move whose parent node is not in the op's causal history. Such a malformed update used to be applied without the parent check; with a subscriber the event computation then panicked and trapped the WASM instance, leaving the document unusable. `import` and `importJsonUpdates` now throw a decode error and leave the document unchanged. Updates that name a parent the importing document merely has not received yet are unaffected: the parent only has to be in the op's own history, so every peer decides the same way.
