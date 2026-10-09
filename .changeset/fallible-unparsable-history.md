---
"loro-crdt": minor
---

Return decode errors when fallible history operations first read an unreadable change block, preserving document state instead of panicking during lazy DAG loading.

Keep attached imports atomic even after all DAG headers have been loaded: unparsed change-block bodies still require rollback protection. Share the existing version snapshot between rollback journals to avoid two extra copies of the version vector. Legacy infallible range readers continue skipping only the damaged block while recording the failure.

`state_vv` / `version`, JSON update export, cursor lookup, `apply_diff` rollback, shallow imports, and `fork_at` return `DecodeError` instead of panicking or wrapping the failure as `Unknown`. `state_vv` and `export_json_updates` now return `LoroResult`. `try_state_vv`, `try_export_json_updates`, and `try_get_cursor_pos` report the same failure. `CannotFindRelativePosition` is `#[non_exhaustive]` and gains `HistoryUnreadable`. `LoroEncodeError` gains `DecodeError`.

Rust compatibility: `ChangeTravelError` gains `HistoryUnreadable(LoroError)` and is now `#[non_exhaustive]`; downstream exhaustive matches must add a wildcard arm. `FrontiersNotIncluded` changes from a unit struct to a struct with a private field plus a same-named constant. Construction via that constant still compiles, but an exhaustive `match` of `Err(FrontiersNotIncluded)` does not (`E0004`): the `history_error: Some(_)` pattern cannot be written from another crate (`E0451`) because the field is private. Downstream must use `Err(_)`. An unreadable-history error is not equal to the `FrontiersNotIncluded` constant.
