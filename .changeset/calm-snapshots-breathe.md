---
"loro-crdt": patch
---

Reduce peak memory across large snapshot import, `toJSON`, update import, and
snapshot export by preserving lazy state, bounding the decompressed SSTable
block cache by bytes, and preallocating the snapshot output exactly. Snapshot
export now also rejects inconsistent alive-container parent metadata instead of
persisting it. Snapshot import now validates every embedded SSTable block
checksum before accepting external bytes.
