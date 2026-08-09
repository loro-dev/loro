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
