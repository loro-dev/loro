---
"loro-crdt": patch
---

Stop the document's arenas from growing every time a change block is parsed again after an export. Streaming text with frequent exports made the string arena grow quadratically until offsets overflowed past 4 GiB and the export panicked (`append-only-bytes` `start <= end`), poisoning the document. List values grew the same way. Parsed blocks now keep their text and list values in buffers freed with them.

Keep pending changes when a snapshot is imported into a document that holds only pending changes; they were dropped before.
