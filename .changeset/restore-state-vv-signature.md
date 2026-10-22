---
"loro-crdt": patch
---

Restore the Rust `LoroDoc::state_vv()` return type to `VersionVector`, preserving
source compatibility with versions before 1.17.0. Keep `try_state_vv()` as the
fallible API for callers that need to handle unreadable history. The legacy
method panics on resolution failure after releasing the document locks. WASM
`version()` continues to return readable errors through the fallible API.
