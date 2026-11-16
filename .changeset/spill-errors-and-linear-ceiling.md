---
"loro-crdt": patch
---

`tryReclaimGlobalToBudget()` no longer fails forever after WebAssembly memory grew past `maxWasmLinearBytes`: like `tryCommit()`, it succeeds when the live heap is back under 70% of the ceiling. `globalMemoryStats()` adds `wasmLinearMemoryExceeded`, which stays true once the pages have grown past the ceiling (they never shrink).
