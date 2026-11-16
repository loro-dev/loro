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

## #1299: completed subset

Rebased onto the repaired #1296; conflicts combined blob-size preflight with the
owned metadata reader and typed-array views, preserving both behaviors.

| Review item | Result |
| --- | --- |
| Clock sweep skips entries | Fixed: use one starting hand for both passes. New test covers 5/100/1000 containers and hands 0/3, verifies no full states remain and contents/editing survive. |
| Revert frontier resolution defaults to empty vv | Fixed: use `frontiers_to_vv_or_not_found` and propagate the error. |
| Revert failure leaves scope open / batch rolls history back ahead of state | Still pending. Fixing this needs a rollback plan prepared before applying imported state, or a state journal; returning another error or rolling history back unconditionally cannot preserve atomicity. No claim that these branches are fixed. |
| Soft-only imports never reclaim | Fixed: reclaim after single/batch import rollback scopes close, allowing actual history compaction. New integration test covers both entry paths with no hard doc quota. |
| O(document) budget statistics / repeated unreclaimable sweeps | Pending: page statistics depend on the sparse pages introduced in #1300. No page cache or backoff change is in this branch. |
| Undo items with unknown affected containers | Pending: the empty set can mean unknown rather than no containers. Needs conservative filtering plus a same-peer checkpoint regression. |
| Unlimited undo byte estimation | Fixed: no byte walks when quota is `usize::MAX`; operation-count test with 1000 entries checks zero size walks. Finite-quota eviction still recalculates and needs incremental accounting. |
| Outdated import bypasses hard limit | Not applicable: both `decode_oplog_changes` variants and `decode_snapshot_bytes` reject outdated modes with `ImportUnsupportedEncodingMode` before mutation. |
| Quota installed during pending edits or commit callback | Fixed with user-confirmed rule: pending transactions retain their captured hard doc quota; the new quota applies to the next transaction. Empty auto-commit transactions adopt it before edits. Rust + JS regressions cover preserved edits, next-transaction rejection, rollback and subsequent editing; Rust also covers pre-commit callback installation. |
| Gap between try_commit measurement and commit | Fixed: shared commit path performs admission, rollback and commit under the transaction lock. `apply_diff` and manual commits use the captured quota too. An active nested batch rejects concurrent try_commit instead of disturbing its scope. |
| Adding budget scope inside another batch | Guarded in document budget arming: do not insert under active nested scopes. Global setter in #1301 still needs to adopt this rule. |

Validation executed on the completed subset:

- `cargo test -p loro --test memory_budget --test callback_panic --test apply_diff_atomicity`: 12 + 3 + 16 passed.
- `cargo test -p loro-internal --lib full_clock_sweep_visits_every_cold_container`: passed.
- `cargo test -p loro-internal --lib unlimited_undo_trimming_does_not_walk_history`: passed.
- `pnpm test-loom`: 10 passed; this is the existing concurrency suite, not a new exhaustive budget interleaving model.
- `CI=true pnpm release-wasm`: 58 Vitest files, 574 passed (2 skipped, 2 todo); declarations, CommonJS, Deno 4 and Bun 4 passed.
- `cargo fmt --all -- --check`, `git diff --check`: passed.

## Still outstanding

This is not the complete four-PR repair. The design issue currently left in #1299
is failed state restoration during a rejected import: a precomputed inverse or a
state journal must retain atomicity without reintroducing whole-document copies.
Those paths must be addressed before treating the hard-quota work as ready.

#1300 and #1301 have not been rebased or changed in this continuation. Their
remaining review items are:

- #1300: actual buffer release after spill (metadata part fixed below), fallible
  spill-directory switching and retries, incremental flush instead of whole-table
  rewriting, immutable spill sharing on clone, Unix 0600 files, fork budget
  inheritance, shallow-root backend propagation, boxed-op memory accounting,
  string-arena shrink copying, deterministic DAG work-count test, stale context.
- #1301: WASM pressure hysteresis, registry strong-reference/drop race,
  O(document-count) global accounting, global setter rollback-scope guards,
  consistent linear-memory recovery verdict, conservative UTF-8 preflight,
  global cache buffer retention on document drop, handle-read LRU tracking,
  duplicated detailed-batch branches, repeated Stage 1 during Stage 2.
- Performance investigation: page accounting + global heap fast path, all text
  methods/package entries with short-string direct binding and large-input
  preflight, bounded JSON cache acceptance, and combined checkout block lookup.
  Existing prototype timing is not validation of any change in this report.

Remote heads before this continuation were verified with `git ls-remote`:
`3e3eb6fb670e1c0c550fcc30992e5aa9b0e322eb`,
`1b8c4dc4131c1bf69e939f36bb32a286ac17ab69`,
`35510eff1dfec1a11bd4344064c17c74de5f8946`,
`4cefe825b2a26db3bfd75cb5b602afab8af32e29`.

#1296 was pushed at `d2e88876`. Final pushed heads are reported in chat; no merge
or deployment was performed.
