---
"loro-crdt": minor
---

Add `toContainerTree()` to documents and attached containers. It returns independent recursive `{type, cid, value}` nodes and opaque ordinary `Value` nodes. Documents can select visible roots without creating missing roots. Text formatting applies to all descendants and is inferred in TypeScript. List and MovableList expose `toContainerTreeSlice(start, end)` with explicit `start`, `totalLength`, and `items`. Fixed JS construction, per-read key/peer reuse, and owned binary buffers avoid a public transport protocol.

Optional text configurations retain the default plain-text possibility in TypeScript. Literal root selections preserve their names as optional result properties.

Compile Node snippets to CommonJS so the package does not require `require(esm)` support.
