# Memory PR repair progress

Verified against code 2026-11-16.

This records actual repairs to the stack #1296 -> #1299 -> #1300 -> #1301.
The original performance investigation is in the sibling worktree's
`benchmarks/pr-memory-20261116/causes/ROOT_CAUSES.md`; its timings are not
measurements of these repairs.

## #1296

| Review item | Result |
| --- | --- |
| Frontier block body validation | Restored `get_change` validation at snapshot import; header existence alone is insufficient. Uses existing malformed-history coverage; no new malformed fixture. |
| Metadata keys retain old input | Fixed: copy only first/last keys on metadata decode. New owner-drop test proves metadata no longer owns the input buffer. |
| Raw export bypasses tombstone cleanup/coalescing | Fixed conservatively: normal blocks use `add_new_block`; large blocks still copy encoded bytes. New tombstone and gap-block tests pass. Export timing still needs remeasurement. |
| Ordinary export clears parsed cache | Fixed: separate flush from explicit compaction. New repeated-export test checks warm parsed representation and stable bytes. |
| freeHistoryCache discards tree import index | Fixed: release only checkout index; `free_all` remains for rollback/reset. New cache-lifecycle test. |
| JS typed-array constructor copies inputs | Fixed: validate typed-array references and read/copy views directly. New small/streaming subarray tests preserve input and contents. |
| Decompressed Vec capacity exceeds accounting | Fixed: convert to boxed slice before Bytes so decompression spare capacity is released. |
| Owned batch metadata copies input | Fixed: both owned batch APIs use `decode_import_blob_meta_bytes(blob.clone(), false)`. Existing owned/slice parity tests pass. |
| Stale context | Updated cache lifecycle and lazy decode docs; frontier parsing documentation becomes true again. Historical measurements are explicitly labeled. |
| Duplicated mode/span code and unrelated dead wrappers | Deferred: cleanup does not fix the measured regression; avoid mixing broad refactoring into behavior repairs. |
| Two WASM copies in streaming batches | Retained: one metadata pass plus one on-demand import pass, keeping payload residency bounded. Removed extra JS heap copies, not these two WASM passes. A metadata-only JS reader needs separate design. |
| Empty document allocation | Fixed structurally: one lazy `Box<FlushedMeta>` instead of two inline optional values; no new timing claim. |
| JSON cache / checkout probe optimization | Pending; investigation candidates have not established no regression. |

Validation executed:

- `cargo check -p loro-internal -p loro-kv-store`: passed.
- `cargo check -p loro-wasm --target wasm32-unknown-unknown`: passed.
- `cargo test -p loro-kv-store --lib`: 35 passed.
- `cargo test -p loro-internal --lib oplog::change_store::test`: 23 passed.
- `cargo test -p loro --test import_batch_bytes --test snapshot_import_allocation --test fork_at_deleted_container`: 20 passed.
- `CI=true pnpm release-wasm`: passed; declarations/CommonJS checks, Vitest 569 passed (2 skipped, 2 todo), Deno 4 passed, Bun 4 passed.
- `cargo fmt --all`, `git diff --check`: passed.

Dependency setup initially failed without a TTY, then offline cache lacked a
package, and sandboxed npm downloads failed. Installation and release validation
succeeded with the required sandbox escalation. No dependency versions changed.

## Upper layers

#1299, #1300 and #1301 review items and performance fixes remain pending until
recorded below. No merge or deployment is part of this work.
