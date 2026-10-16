# loro.js Concurrent Tree Moves

Verified against code 2026-10-17.

## Effective moves, not independent parent registers

`loro-js/src/runtime/tree-moves.ts` (`TreeMoves`) applies tree operations in
ascending `(lamport, peer)` order, matching Rust's `TreeCacheForDiff::apply`
(`crates/loro-internal/src/diff_calc/tree.rs`). A move whose new parent is its own
node or a descendant stays in history but does not change the parent, position,
writer, or last effective move ID. Deletion stops the ancestor walk, as it does
in Rust. Creating a new node and reordering within the same parent need no
ancestor walk.

For issue #1182, concurrent A→B and B→A must leave B at the root with A below it
when A→B sorts first. Moving A back to the root must leave both nodes at the root:
the previously skipped B→A never became B's placement. Undo also restores the
previous *effective* placement, rather than the preceding raw operation.

A later import can sort before a move already applied locally. Checking only
the incoming move would choose a result based on delivery order. `TreeMoves`
therefore retains each applied operation's previous effective placement,
retreats the affected writer suffix, and replays it in order. Checkout uses the
same mechanism, including operations on other nodes whose cycle checks can
change. Change-level ordering alone is insufficient: operations inside one
change can interleave with another peer's operations by lamport.

## State, historical queries, and events

`LoroDoc.#ensureTreeMoves` reconstructs a lazily hydrated tree's move history at
its current version, starting from the shallow root state if present. Normal
local edits append to the cache; they do not rebuild the tree. `LoroTree._reset`
drops the cache, and failed imports invalidate registrations before restoring
state, so a rejected import cannot poison a later reuse of those operation IDs.

`#installTreeMoves` writes the resolved placements through the existing sibling
indexes. It captures old event positions for every affected node before changing
any records, including a previously applied move that the incoming history makes
ineffective. The existing tree diff sequencing remains responsible for producing
applicable, parent-first events (see [loro-js-tree-diff.md](loro-js-tree-diff.md)).

`TreeMoves.placementAt` temporarily resolves another version and restores the
current one. It caches an overlay of only the affected nodes for historical
visibility checks. A diff for one recent move must not copy all tree nodes or
scan all earlier tree operations. Per-peer counter indexes locate the changed
writer window; subsequent operations in that window are replayed because their
cycle checks depend on the earlier operations.

Snapshot encoding and local fractional-index allocation keep the existing
ordering rules from #1108 and #1148. The regressions live in
`loro-js/tests/tree-concurrent-moves.test.ts`, alongside the unchanged
`tree-snapshot`, `tree-local-positions`, and `tree-diff-order` suites.
