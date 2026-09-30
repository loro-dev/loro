---
"loro-crdt": patch
---

`export({ mode: "updates-in-range" })` no longer panics (a `RuntimeError: unreachable` trap in WASM) when the spans hold several non-contiguous or overlapping ranges of one peer; all the ranges are exported.
