---
{}
---

Add Rust `LoroDoc::from_snapshot_bytes(bytes::Bytes)` to import snapshots with shared ownership of an existing buffer, avoiding an additional full-buffer copy. Preserve validation before copying in the borrowed `from_snapshot` API. Integrity checks still scan the snapshot during import; memory mapping does not make file reads fully lazy. Re-export the `bytes` crate as `loro::bytes`. The document may retain the input buffer for its lifetime, so a memory-mapped file must not be modified in place while the document is alive.
