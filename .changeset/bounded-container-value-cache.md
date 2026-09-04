---
"loro-crdt": patch
---

Fix wasm memory retention when reading a document container by container
(loro-dev/loro#1092).

Every read through a container handle (`LoroMap.keys()`/`get()`,
`LoroList.get()`, `LoroText.toJSON()`, ...) decoded the container's value into
an in-memory cache that was pinned for the lifetime of the document — about
4 KB per container, released only by `doc.free()`. Walking a large document
this way (the pattern loro-mirror's initial state build uses) retained ~4 KB ×
containers-ever-read and trapped wasm32 at the 4 GiB limit around one million
containers.

The decoded-value cache is now bounded (2048 entries, second-chance FIFO).
Evicted entries are pure caches over the KV store and are re-decoded on the
next read, so this only changes memory behavior, not API semantics.
`doc.free()` semantics are unchanged.

Measured on the issue's repro (570-turn document, 188k container handles,
release build): the handle walk retains no per-container memory (external
memory flat at ~84 MiB vs +641 MiB before) and runs ~10x faster
(1.28 s vs 12.5 s) thanks to the smaller working set.
