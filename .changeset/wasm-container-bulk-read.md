---
"loro-crdt": minor
---

Add bulk deep-read APIs on containers. `LoroMap`/`LoroList`/`LoroMovableList`/`LoroTree`/`LoroText` now expose `getDeepValueWithID()` returning the same `{ cid, value }` node shape as `LoroDoc.getDeepValueWithID()`. `LoroList` and `LoroMovableList` also expose `getRangeDeepValueWithID(start, end)` and `getRangeValue(start, end)` for reading a slice of a list in one WASM call; bounds are clamped (negatives to 0, overflows to the length) and empty or inverted ranges return `[]`. Detached containers throw a readable error instead of trapping.

Potentially breaking: the `cid` field in `getDeepValueWithID()` results is now the bare container id string (e.g. `cid:92@2311024965712536503:Map`, `cid:root-map:Map`) — exactly what the container's `id` property returns. Previously it was a Debug-style composite like `idx:85, id:cid:92@2311024965712536503:Map`. Update any consumer that parsed or matched the old `idx:N, id:...` shape.
