# Rich-Text Insert Positions and Delete IDs

Verified against code 2026-11-26.

## Where local text goes next to style anchors

`RichtextState` (`crates/loro-internal/src/container/richtext/richtext_state.rs`)
keeps every style's start and end anchors in the text. An anchor has entity
length 1 and length 0 in bytes, Unicode, UTF-16, and event indexes, so one user
position can name several entity positions: the gaps of the run of anchors
between the two displayed neighbours (`locate_insert_gap`).

### Local text inserts (loro-dev/loro#1240)

`TextHandler::insert` (attached, through `insert_with_txn_and_attr`, and
detached) uses `get_entity_index_for_local_text_insert`. The rule is stated on
what the user sees, not on anchors. Per style key, with L and R the displayed
neighbours and their resolved values (`resolve_styles`: greatest
`(lamport, peer)`, null means absent), the new text should get
(`desired_insert_styles`):

1. the value L and R share;
2. else L's value if the style op it comes from expands after;
3. else R's value if its op expands before;
4. else nothing.

So nothing comes from a style no displayed character carries (an unmark leaves
no value, and a deleted character is not a neighbour), and two documents that
display the same styles give the same result whatever their hidden anchors.

`plan_insert_in_anchor_run` walks the run's gaps, keeping the styles that cover
each (L's styles, plus a start anchor's style, minus an end anchor's), and
picks a gap whose resolved values equal the desired ones, the one nearest the
legacy pick below (ties go left). If no gap matches (concurrent anchors in an
unlucky order, e.g. `bold` start before `sup` start where only `sup` is
wanted), it takes the gap with the fewest wrong keys and returns
`InsertStyleFix`es: after the insert op, the handler writes a mark (the
desired op's value and expand flag) or an unmark (the wrong op's flag,
`to_delete`) over exactly the new text (`mark_with_txn_and_flag`). An insert
with attributes (`apply_delta`) sets every key itself and writes no fixes.
`PosType::Entity` names one exact position and keeps the legacy rule.

Why the old anchor-only rule was wrong (all four shapes of #1240): it used one
stop for every key, so an anchor of one key carried the text past another
key's anchor; it treated anchors of unmarked or deleted styles like live ones,
and an unmark is a style with the expand reversed (`ExpandType::reverse`), so
after unmarking a both-expanding style the scan stopped inside the original
pair; and two adjacent marks with one value leave a gap between the first's
end and the second's start that one mark does not have.

### Legacy rule (mark endpoints, `PosType::Entity`, tie-break)

`find_best_insert_pos` picks one gap by anchors alone:

1. Scan the anchors after the previous character from left to right. Stop at
   the first anchor that new text stays in front of
   (`insert_stays_before_anchor`): the start of an unmark (null or false
   value), or an anchor that prefers inserts before it
   (`TextStyleInfoFlag::prefer_insert_before`: a start anchor that does not
   expand before, an end anchor that expands after).
2. Walk back over the scanned anchors while they prefer inserts before them.

Both ends of `TextHandler::mark` go through `get_entity_index_for_text_insert`,
which applies this rule. loro.js mirrors both: `LoroText.#insertPosition`
(legacy) and `LoroText.#localInsertPlan` (local inserts).

## The insert cursor cache

`get_entity_index_for_text_insert` first tries the cursor left by the previous
lookup or insert (`try_get_cached_cursor`), so consecutive typing skips the
tree query. The cache is only used where the anchor rules cannot move the
insert (`anchor_rules_may_move_insert`):

- the text has no style;
- the cursor is inside a text chunk, between two text chunks, or at the start
  or end of the text;
- the cursor is at the end of a text chunk and the next element is an anchor
  that new text stays in front of. The scan stops at that anchor, so the answer
  is the cursor itself; this is typing at the end of a bold range.

A cursor on an anchor, or at the start of a text chunk right after an anchor,
falls back to `find_best_insert_pos`. Local text inserts
(`get_entity_index_for_local_text_insert`) use the cache only inside text,
between text chunks, or at a text end; at the end of a text chunk followed by
anchors they plan from that run directly, without the tree query. Typing continues from the end of the new
text, which is usually followed by text or by an anchor the text stays in front
of, so typically only the first keystroke next to an anchor pays for the query.
Before loro-dev/loro#1135 the cache was trusted at
every chunk boundary, so the side of an anchor that text landed on depended on
the previous lookup (a no-op `unmark` could move the next insert into a style).

The cache stores one surviving leaf and five inline prefix indexes. A read
miss preserves the cache. A partial single-leaf text deletion reuses the
leaf and records its event-coordinate and entity prefixes. A removed leaf,
a style anchor deletion, or a multi-leaf delete clears the cache. A later
insert still checks the anchor rules before using the cached cursor.

- Test: `richtext_state/insert_pos_cache_test.rs` compares every cached answer
  with a cold lookup for all five `PosType`s on random documents (all expand
  types, overlapping styles, null/false values, adjacent and empty anchor
  pairs, astral/ZWJ/flag text). It also compares randomized UTF-16 replacements
  with a cold cursor and a new document replayed from full updates.
  `LORO_INSERT_CACHE_SEEDS=<n>` runs more seeds for the position lookup test.
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

## Static delta reads versus incremental events (#1241)

Verified against code 2026-11-12.

`state::richtext_state::RichtextState::get_delta` and the detached branch of
`TextHandler::get_delta` filter null style values before comparing neighboring
runs, using `StyleMeta::to_option_map_without_null_value`. A removed style and an
absent style represent the same static state. `get_richtext_value` already does
this through `StyleMeta::to_value`. Keep the null-valued anchors in the document,
and keep `to_option_map` in incremental diff/event conversion: a retain with
`{ bold: null }` tells the consumer to remove bold. Regression coverage lives in
`tests/richtext_unmark_runs.rs` and WASM `tests/richtext.test.ts`.
