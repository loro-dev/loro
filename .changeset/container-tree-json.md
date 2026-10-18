---
"loro-crdt": patch
---

Add `LoroDoc.toContainerTreeJson(options?)` to read visible container trees as one JSON string built in Rust, without per-value WASM-to-JS calls. `JSON.parse` returns the same node shapes, IDs and recursively selected text format as `toContainerTree`, with the same root filtering and no implicit commit. Binary values and non-finite numbers are rejected rather than silently changed. Existing `toContainerTree` behavior and return types are unchanged.
