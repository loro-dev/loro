---
"loro-crdt": patch
---

Clarify map-child deletion undo in the package README and Rust `UndoManager` docs.
Regular children are reconstructed with new IDs, so concurrent edits to the old
child do not appear in the restored child. Mergeable children preserve identity
and concurrent edits when their parent identity is unchanged. Explain retained
state on slot reuse, the recreated-ancestor boundary, and using a visibility or
deleted field for entity deletion that must preserve identity. Add regression
tests for both import orders. No undo or CRDT merge behavior changes.
