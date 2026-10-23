---
{}
---

Fix Rust `loro-delta` text insertion dropping the final chunk of strings longer than 128 bytes. Share UTF-8-aware chunking between append and middle insertion, preventing capacity panics when a multibyte character crosses a chunk boundary.

Preserve insertion order and existing text attributes when a partial UTF-8 chunk merges with a neighboring B-tree leaf. Keep remaining batch items after any prefix merged at the end of the tree.

Make `loro-delta` insert batches in order through single-item B-tree insertion, so the published crate is correct with its existing registry dependency on `generic-btree` 0.10.7 and does not require the workspace-only B-tree patch.
