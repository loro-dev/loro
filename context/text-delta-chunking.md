# Text delta chunking and batch insertion order

Verified against code 2026-10-23.

`crates/delta/src/text_delta.rs` uses `TextChunk::from_long_str` for long
appends and middle insertion. Chunk capacity is measured in bytes (128 normally,
8 in unit tests); splitting must stop on a UTF-8 boundary and include the final
slice. A valid chunk can be smaller than capacity even with more input remaining.

Middle insertion reaches `DeltaRope::insert_values` and then
`BTree::insert_many_by_cursor` in `crates/generic-btree/src/lib.rs`. The batch
can merge its prefix into the leaf immediately before the insertion point.
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
