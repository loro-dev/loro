# Text delta chunking and batch insertion order

Verified against code 2026-11-18.

`crates/delta/src/text_delta.rs` uses `TextChunk::from_long_str` for long
appends and middle insertion. Chunk capacity is measured in bytes (128 normally,
8 in unit tests); splitting must stop on a UTF-8 boundary and include the final
slice. A valid chunk can be smaller than capacity even with more input remaining.

Middle insertion reaches `DeltaRope::insert_values`, which buffers multiple
items and inserts them back to front using `BTree::insert_by_path`. Each returned
cursor marks the start of the inserted item, so the next item is inserted before
it. Empty and single-item inputs avoid allocating a batch buffer. The minimum registry dependency is `generic-btree` 0.10.8, published separately
with the in-tree fixes. The root `[patch.crates-io]` does not propagate to
downstream packages; `scripts/cargo-release.ts` excludes this independently
versioned crate from the Loro workspace release.

The underlying defect is in `BTree::insert_many_by_cursor` in
`crates/generic-btree/src/lib.rs`. Its batch can merge a prefix into the leaf
immediately before the insertion point.
`split_leaf_if_needed` must report the current leaf as `left_neighbour` when
the cursor offset is nonzero: it remains the left half after an interior split,
or precedes the insertion at its end. Only offset zero uses the previous sibling.
Choosing the previous sibling for an interior cursor can silently move inserted
text before existing content. Partial UTF-8 chunks make this reachable because
they can fit into a short preceding leaf; full-capacity ASCII chunks hide it.

At the end of the tree, `new_pos` is absent. If merging consumed a batch prefix,
the remaining items must use the current last leaf's updated length rather than
the original cursor offset, or the remaining items appear before the prefix.

Regression coverage: `utf8_middle_insert_preserves_order_and_existing_attributes`
and `repeated_utf8_middle_insert_matches_string` in the delta integration tests;
`batch_insert_preserves_order_at_leaf_boundaries` in the generic B-tree rope
tests checks leaf boundaries, parent splits, caches, and subsequent insertion.

`bash scripts/test-delta-published-deps.sh` packages the actual `loro-delta` crate,
extracts it outside the workspace, pins the registry B-tree dependency to 0.10.8,
and runs all package tests. CI runs this independently of workspace tests; pass
`--offline` to use locally cached dependencies. Do not remove the delta-level
ordered insertion merely because the in-tree B-tree regression tests pass.

## Deletion composition boundaries

`DeltaRope::_compose_replace` in `crates/delta/src/delta_rope/compose.rs`
locates a nonempty deletion's end by querying its last consumed output unit
(`range.end - 1`) and incrementing that cursor's offset. A plain end query uses
the right-biased `LengthFinder`, which skips zero-output deletion fragments at
the boundary. Those fragments are outside the output being deleted but would
be visited by `BTree::update`. Repeated backward deletions accumulated one such
fragment per call and scanned the suffix again, causing quadratic work.

The new cursor stops at the end of the last consumed retain/insert. Existing
deletions after it stay intact. Zero-output fragments strictly inside the range
remain part of the update, as required. Multi-leaf pure deletions collapse
accumulated deletion fragments into a single `Delete` leaf (or fold into an
immediately preceding `Replace` leaf) via `_delete_batch_leaves`, and single-leaf
deletions at offset zero of a `Retain` leaf merge into a left-neighbour
`Delete`/`Replace` leaf so repeated fixed-offset deletions do not accumulate
zero-output prefix leaves either.

`repeated_backward_deletes_do_not_rescan_deleted_suffixes` and
`repeated_fixed_offset_deletes_before_suffix_coalesce_fragments` count
value-length reads rather than elapsed time, bounding work across 4,096
compositions.
Integration tests compare boundary replacements and 8,192 seeded replacements
against sequential string edits. The public undo regression includes text plus
a final map write, two texts per commit, and list pushes, with concurrent remote
insertion positions and repeated undo/redo.
