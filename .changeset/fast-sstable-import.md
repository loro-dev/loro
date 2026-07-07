---
"loro-crdt": patch
---

Speed up snapshot import. When decoding a Loro snapshot, the redundant per-block SSTable validation (eager block-metadata decode and per-block checksums) is now skipped, because the whole snapshot body is already protected by the document-level checksum verified during decoding. This removes a second hash pass over the data (roughly halving B4 snapshot import time) while preserving integrity guarantees.

This fast path is internal to Loro's snapshot decoding. The public `MemKvStore::import_all` still verifies every block's checksum; a separate `import_all_unchecked` opts into the unchecked path and is only used where an outer checksum already guarantees integrity.
