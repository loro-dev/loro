---
"loro-crdt": patch
"loro-crdt-map": patch
---

Speed up `export({ mode: "shallow-snapshot" })` by up to ~20x on container-heavy documents with a large retained history.

Building the state at the shallow root used to check the live document out
backwards (latest -> root). That reverse diff makes the richtext/list diff
calculators rebuild a full CRDT tracker from empty for every container touched
in the range, which dominated the export cost: on a doc with ~66k containers
and ~720k streaming-edit ops, shallow export at a mid-history root took ~3.2s
versus ~1.5ms for a full snapshot. When at least 64k ops are retained since
the root, the root state is now reconstructed by replaying the pre-root
history forward into a temporary doc, and the latest state is read from the
live store directly without moving the document. The pre-root prefix is bounded
both relative to the tail (at most 16x the retained op count) and absolutely
(at most 1M ops), so a document whose pre-root history is huge and unrelated to
the tail keeps the previous checkout path. The same export drops to
~370ms and the produced blob is slightly smaller (~17% on the same fixture).
With a small retained range — including a root at the latest version — the
previous checkout path is kept: it is then equally fast and peaks at ~4x less
memory, so exporting a lazily imported document no longer materializes its
whole state. Exported blobs remain logically equivalent; detached or
already-shallow source docs also keep the previous code path.

Also fixes shallow snapshot export resurrecting, as an empty entry, a root
container deleted with `deleteRootContainer` before the shallow root.
