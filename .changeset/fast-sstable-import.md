---
"loro-crdt": patch
---

Speed up snapshot import. On fast imports the per-block SSTable validation (eager block-metadata decode and per-block checksums) is now skipped, because the whole snapshot body is already protected by the document-level checksum verified during decoding. This removes a redundant second hash pass over the data (roughly halving B4 snapshot import time) while preserving integrity guarantees.
