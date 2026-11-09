---
"loro-crdt": minor
---

Stop caller input and untrusted updates from panicking, trapping, aborting the process, or silently losing data. The Rust crates `loro` and `loro-internal` move to 1.18.0 with the same changes.

**Writes can now be rejected.** A local write (`insert`, `set`, `commit`, `applyDiff`, `revertTo`, undo) throws instead of forking history that the doc already depends on:

- `ConcurrentOpsWithSamePeerID`, in detached editing, when the peer id already has ops after the current version.
- `ConcurrentOpsWithSamePeerID`, in any mode, while updates are waiting for a dependency (`pending` in `ImportStatus`) and they use the doc's peer id from its next counter on, or depend on such an id. Before, the write succeeded and importing the dependency later corrupted the document or aborted the process.
- Rust only: `DuplicatedTransactionError` while a transaction opened through `LoroDoc::inner().txn()` is uncommitted.

To recover: import the missing dependency, after which the waiting updates apply and writes work again, or switch to a fresh peer id with `setPeerId`. A remote can park an update under your peer id to block writes with it; a new peer id is the way out. In Rust, commit the open transaction and retry.

**Imports can be rejected while a transaction runs.** Rust only: `import`, `import_json_updates` and `import_batch` return `ImportWhenInTxn` while a transaction opened through `LoroDoc::inner().txn()` is uncommitted, and when called from an event or pre-commit callback of the same document. Commit the transaction, or import after the callback returns. Starting a transaction from such a callback returns `DuplicatedTransactionError` instead of deadlocking.

**`import` returns `UsedOpID` for conflicting data that brings no new ops.** Bytes whose ops reuse ids the document already has with different content used to return `Ok` and drop them silently. They now throw `UsedOpID`, as do updates that conflict with updates still waiting for a dependency, and a single update that contradicts itself. Importing what the document already has, or already holds as pending, is still a no-op. Data relayed through JSON can differ in representation only (numbers, binary, non-finite doubles): `importJsonUpdates` accepts that, and in Rust so do `import_with_history_mode` and `import_batch_with_history_mode` with `ImportHistoryMode::JsonLossy`.

**`importBatch` is not all-or-nothing.** When one update in a batch fails on its own, the batch throws and keeps what the other updates imported, including a dependency that lets pending updates apply. Only a failure of the final checkout rolls the whole batch back. An error from `importBatch` does not mean nothing was applied, and it does not say which update failed.

**New `importBatchDetailed` reports each update of a batch.** It imports as `importBatch` does and, instead of throwing for an update that fails, returns `{ blobs, status, rollbackError }`. `blobs[i]` is the outcome of the i-th update you passed:

- `{ type: "imported", status }`: the update is in the document. `status` is the `ImportStatus` of that update: its `pending` ops wait for a dependency, which a later update of the same batch may have brought.
- `{ type: "failed", error }`: the update failed on its own, for example with `UsedOpID` or a checksum mismatch. The other updates are not affected.
- `{ type: "rolledBack" }`: the update imported, but the final checkout failed and the whole batch was rolled back.

`status.success` is everything the batch imported and `status.pending` is everything the document still waits for when the batch ends. `rollbackError` is `null` unless the final checkout failed. Then it holds the error, the document is as it was before the call, no entry is `imported`, and no event is emitted. The checkout checks the batch as a whole, so it cannot say which update it rejected: import the updates one at a time to find it. Updates that failed on their own keep their `failed` entry in that case.

Two differences from `importBatch`: an update whose metadata does not decode (`decodeImportBlobMeta` throws for it) is `failed` and the others are imported, where `importBatch` throws before it imports anything. When every update fails that way, nothing is imported and the document is left as it was, including an uncommitted local change. And a batch of one is a plain import, so the state checks it at once and its failure is `failed`, never `rolledBack`. In Rust this is `LoroDoc::import_batch_detailed` and `import_batch_detailed_with_history_mode`, returning `ImportBatchDetail` with one `ImportBlobOutcome` per blob. They return `Err(ImportWhenInTxn)` when the call is refused as a whole, as `import_batch` does. Nothing was tried then.

**Rust: `UpdateTimeoutError` gains `Loro(LoroError)`.** `LoroText::update` and `update_by_line` return the edit's own error, such as a deleted container or a read-only document, instead of panicking. Exhaustive `match`es on `UpdateTimeoutError` need the new arm.

**New and deprecated APIs.**

- Rust: new `LoroDoc::try_attach` and `LoroDoc::try_checkout_to_latest` return the error, and the doc stays detached at its version and usable. `attach`, `checkout_to_latest` and `find_id_spans_between` are deprecated in favor of `try_attach`, `try_checkout_to_latest` and `try_find_id_spans_between`. They still panic on that input, now after releasing the document's locks.
- JS: `attach()` and `checkoutToLatest()` now throw where they trapped the WASM instance. `findIdSpansBetween` already threw. No JS API is deprecated.
- New `importBatchDetailed` (Rust: `import_batch_detailed`, `import_batch_detailed_with_history_mode`, `ImportBatchDetail`, `ImportBlobOutcome`), described above.
- Rust: `checkout` returns `TransactionError` while a transaction opened through `inner().txn()` is uncommitted, where it used to panic.
- `export({ mode: "snapshot" })` of a detached document throws when its latest version cannot be checked out, where it panicked.

**Other fixes.**

- `checkout` and `forkAt` accept frontiers with several ids of one peer or repeated ids, and use the greatest id of each peer. `decodeFrontiers` does the same for bytes that repeat a peer.
- `LoroText.update` and `updateByLine` throw the edit's error, such as a deleted container, instead of trapping.
- Frontiers, version vectors and id spans that the document does not have (unknown peers, negative or out-of-range counters) no longer panic or poison the document. `checkout`, `forkAt` and `diff` return an error; `frontiersToVV` returns `undefined`, `vvToFrontiers` returns the last id of each peer, and `getChangedContainersIn` covers the part of the range the document has.
- In detached mode, an update that the state would reject (a list insert past the end) makes `attach` and `checkoutToLatest` throw, and the document stays usable.
- A `JsonLossy` import of deeply nested values no longer overflows the stack.
