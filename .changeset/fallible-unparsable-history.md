---
"loro-crdt": patch
---

Return decode errors when fallible history operations first read an unreadable change block, preserving document state instead of panicking during lazy DAG loading.

Keep attached imports atomic even after all DAG headers have been loaded: unparsed change-block bodies still require rollback protection. Share the existing version snapshot between rollback journals to avoid two extra copies of the version vector. Legacy infallible range readers continue skipping only the damaged block while recording the failure.

Rust compatibility: `ChangeTravelError` gains `HistoryUnreadable(LoroError)` and is now `#[non_exhaustive]`; downstream exhaustive matches must add a wildcard arm. `FrontiersNotIncluded` changes from a unit struct to a struct with a private field plus a same-named constant. Existing construction and constant patterns still compile, but an unreadable-history error is no longer equal to the `FrontiersNotIncluded` constant.
