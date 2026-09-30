# Rich-Text Insert Positions and Delete IDs

Verified against code 2026-09-30.

## Where local text goes next to style anchors

`RichtextState` (`crates/loro-internal/src/container/richtext/richtext_state.rs`)
keeps every style's start and end anchors in the text. An anchor has entity
length 1 and length 0 in bytes, Unicode, UTF-16, and event indexes, so one user
position can name several entity positions. `find_best_insert_pos` picks one:

1. Scan the anchors after the previous character from left to right. Stop at
   the first anchor that new text stays in front of
   (`insert_stays_before_anchor`): the start of an unmark (null or false
   value), or an anchor that prefers inserts before it
   (`TextStyleInfoFlag::prefer_insert_before`: a start anchor that does not
   expand before, an end anchor that expands after).
2. Walk back over the scanned anchors while they prefer inserts before them.

`TextHandler::insert` and both ends of `TextHandler::mark` go through
`get_entity_index_for_text_insert`, which applies this rule. loro.js follows
the same rule from loro-dev/loro#1137 on (`LoroText.#insertPosition`).

## The insert cursor cache

`get_entity_index_for_text_insert` first tries the cursor left by the previous
lookup or insert (`try_get_cache_or_clean`), so consecutive typing skips the
tree query. The cache is only used where the anchor rules cannot move the
insert (`anchor_rules_may_move_insert`):

- the text has no style;
- the cursor is inside a text chunk, between two text chunks, or at the start
  or end of the text;
- the cursor is at the end of a text chunk and the next element is an anchor
  that new text stays in front of. The scan stops at that anchor, so the answer
  is the cursor itself; this is typing at the end of a bold range.

A cursor on an anchor, or at the start of a text chunk right after an anchor,
falls back to `find_best_insert_pos`. Typing continues from the end of the new
text, which is usually followed by text or by an anchor the text stays in front
of, so typically only the first keystroke next to an anchor pays for the query.
Before loro-dev/loro#1135 the cache was trusted at
every chunk boundary, so the side of an anchor that text landed on depended on
the previous lookup (a no-op `unmark` could move the next insert into a style).

- Test: `richtext_state/insert_pos_cache_test.rs` compares every cached answer
  with a cold lookup for all five `PosType`s on random documents (all expand
  types, overlapping styles, null/false values, adjacent and empty anchor
  pairs, astral/ZWJ/flag text). `LORO_INSERT_CACHE_SEEDS=<n>` runs more seeds.
- Benchmark: `crates/loro/examples/richtext_typing_bench.rs` (typing next to
  and away from anchors; see its header for how to run it).

## Delete op IDs

A text delete op records its position and the ID of its first deleted character
(`start_id`). `get_text_entity_ranges` splits a local delete into runs that are
contiguous in both entity position and ID, and the handler writes them last run
first. Up to `loro-crdt` 1.16.3 the WASM build tested ID contiguity with the
UTF-16 length, so after an astral character a delete could record a `start_id`
that names other characters (fixed in loro-dev/loro#1135).

Replaying a delete by position (`Tracker::delete`) gives the right text, but
some readers trust `start_id`: placeholders from a shallow snapshot take their
real IDs from it (`CrdtRope::delete`), so a shallow import can end with other
text than a full import, and a cursor on a deleted character that no delete op
names does not resolve. Histories that already contain such deletes are tracked
in loro-dev/loro#1149.

### Existing histories (loro-dev/loro#1149)

Not repaired in code: the op bytes are immutable history, and the readers that go
wrong have nothing but `start_id` to go on. A shallow import's placeholders stand
for trimmed history whose IDs are unknown, and a cursor on a character that no
delete names has no op to follow. Deriving IDs from positions there would mean
seeding the shallow tracker from the root state's per-character IDs, a large
change to diff calculation for a narrow legacy case. Advice for affected
documents (edited with WASM `loro-crdt` ≤ 1.16.3, astral characters, deletes
spanning several insert runs):

- Full imports, full snapshots, and checkouts replay deletes by position and are
  correct.
- Take shallow snapshots at the latest frontiers, or at any version after the
  last delete written by an affected build. The root state then comes from a
  positional replay, and the bad `start_id`s lie before the root.
- Re-create cursors on text that such a delete removed (`getCursor` after
  loading) instead of resolving stored ones, or store positions for these
  documents.
