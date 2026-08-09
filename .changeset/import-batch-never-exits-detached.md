---
"loro-crdt": patch
---

Make `importBatch` atomic and never leave the document detached.

`importBatch` force-detaches the document so each blob only touches the OpLog, then
reattaches with a single checkout at the end. Two exits skipped that reattach and
stranded the document in detached mode, where later imports stop being reflected in
the state, local edits branch off a stale version, and every later `attach()` re-runs
the same failing checkout:

- The closing checkout was `expect`ed to succeed. Remote ops that decode fine but are
  rejected by state validation (for example a list insert past the end of the list)
  only reach that validation here, because the blobs were imported while detached, so
  a malformed blob turned into a panic — a `RuntimeError: unreachable` trap in WASM —
  after the document had already been left detached.
- Any other panic while decoding or applying a blob unwound past the reattach.

The batch now runs inside an OpLog rollback scope. If the closing checkout cannot
apply the accumulated changes, the whole batch is rolled back and `importBatch`
returns the state-apply error with the document still attached and unchanged, instead
of trapping. A panic inside the batch reattaches before it is re-raised.

The pending-changes rollback journal now snapshots each touched slot as it was when
the scope began, instead of undoing individual mutations. The previous two-phase undo
resurrected changes that the same import both parked and unlocked — routine across a
batch — leaving pending entries that referenced container registrations the rollback
had already discarded, and pre-batch pending changes unlocked by a rolled-back batch
are now re-parked instead of silently dropped.

`importBatch` of out-of-order updates is also considerably faster: the import preflight
no longer rescans the whole pending set once per blob, which made such a batch
quadratic. Draining 12000 out-of-order updates in one `importBatch` goes from ~3.4s to
~0.45s, with peak memory unchanged. In-order batches and single `import` calls are
unaffected.
