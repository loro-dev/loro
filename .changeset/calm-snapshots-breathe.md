---
"loro-crdt": patch
---

Reduce peak memory across large snapshot import, `toJSON`, update import, and
snapshot export by preserving lazy state, bounding the decompressed SSTable
block cache by bytes, and preallocating the snapshot output exactly. Snapshot
import now validates every embedded SSTable block checksum before accepting
external bytes.

Full snapshot export no longer walks the alive-container graph: store entries
for referenced containers are created when the creating op or imported diff is
applied, so a cold export after a large import is a flush plus KV export
instead of re-reading every container from compressed blocks. As part of this,
full export round-trips imported state bytes faithfully (inconsistent
alive-container parent metadata is now rejected by shallow export, which still
walks, rather than by full export), and an ensured-but-empty mergeable child is
no longer materialized into the KV store by a full export — importers resolve
it from its parent map marker. First `toJSON` after import also avoids decoding
each lazy container twice.
