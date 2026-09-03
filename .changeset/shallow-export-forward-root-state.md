---
"loro-crdt": patch
"loro-crdt-map": patch
---

Speed up `export({ mode: "shallow-snapshot" })` by ~20x on container-heavy documents.

Building the state at the shallow root used to check the live document out
backwards (latest -> root). That reverse diff makes the richtext/list diff
calculators rebuild a full CRDT tracker from empty for every container touched
in the range, which dominated the export cost: on a doc with ~66k containers
and ~720k streaming-edit ops, shallow export took ~3.2s versus ~1.5ms for a
full snapshot. The root state is now reconstructed by replaying the pre-root
history forward into a temporary doc, and the latest state is read from the
live store directly without moving the document. The same export drops to
~159ms and the produced blob is slightly smaller (~17% on the same fixture).
Exported blobs remain logically equivalent; detached or already-shallow source
docs keep the previous code path.
