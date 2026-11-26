# Undo/Redo of Text Styles Next to Collaborators' Edits

Verified against code 2026-11-26.

## Rule

Per character and style key, on a step whose transformed diff changes a style:

1. The winning style op (highest lamport, then peer) is one of the step's own
   ops: the key gets the value of the latest covering op the step did not
   write, or `null` if none is left. This overrides the transformed diff.
2. The winner is another peer's op: the key is left alone (removed from the
   step's attributes).
3. Otherwise (the step's own peer wrote the winner later, or no op covers the
   character) the transformed attribute stands.

So:

- a collaborator's later mark or unmark of the same key wins over the undo and
  over a redo (loro-dev/loro#1229, both cases);
- text a collaborator inserted, concurrently or later, that took the step's
  style (it lies between the style's anchors) loses it with the rest of the
  range (the undo half of loro-dev/loro#1009);
- a collaborator's own style next to the undone range stays.

Rule 3 exists because undo/redo of deletes re-inserts text as new elements: a
redo of a mark can target such copies, which the original style ops do not
cover. Only the positional diff knows them
(`crates/loro/tests/large_text_time_travel.rs` caught this).

## Where

`LoroDoc::undo_internal_with` (`loro.rs`) calls
`undo::text_styles::restyle_text_diffs` on the transformed step, at the latest
state, before anything is applied. It reads
`RichtextState::undo_style_overrides` (`container/richtext/richtext_state.rs`)
for each text diff that has a retain with attributes; that walks the text chunks
with the style ranges (`StyleValue::value_without`, `style_range_map.rs`).
Inserts and deletes of the step are kept as they are.

## Why not the positional transform

The step's inverse is transformed against the composed remote diff
(`undo::undo`, step 3) with `left_prior = true`, which keeps the inverse's
attributes. Even with priority flipped, a positional delta cannot answer either
question:

- import events report a run's whole style set, not the keys that changed, so
  a remote `{color: blue}` on a run that is also bold comes back as
  `{bold: true, color: blue}`, and dropping its keys drops the undo of bold too;
- a concurrent insert carries the attributes it ended up with; whether they come
  from the undone style or from the inserting peer's own style is only known
  from the ops.

`DeltaRope::transform` (`crates/delta`) now transforms retain attributes as map
writes do (`DeltaAttr::transform`: the side without priority drops the keys the
other sets), which keeps the generic OT convergent, but undo does not rely on it.

## Limits

- Only text diffs that already change a style are rewritten (a cheap check;
  typing undo pays nothing), and only if one of the step's style ops is still
  in the text. If a collaborator deleted all of the undone range and only
  absorbed text is left, the residue stays.
- "Another peer" is any peer but the step's. A collaborator's style op that
  covers a re-inserted copy but is older than the step also wins (rule 2).
- Whether a concurrent insert at a style boundary takes the style is decided by
  peer order (anchors are list elements, `tracker/crdt_rope.rs`), not by the
  expand setting; that is merge semantics and is unchanged. See
  loro-dev/loro#1009.

## Cost

A step that changes a style walks the style ranges once to see if the step's
ops are there, then the text chunks once: O(chunks + style ranges) per text
container, on top of the checkouts that mark undo already does (text marks are
not recorded, see [undo-log.md](undo-log.md)).

Measured 2026-11-26 (release, one undo+redo pair of a 1000-char mark with a
concurrent remote insert, averaged over 40 steps, same binary with the restyle
switched off for comparison): 100k chars / 1k marks 0.6-1.0 ms per step with,
0.7-1.2 ms without (noise); 1M chars / 10k marks 2.45 ms with, 1.5-1.56 ms
without (+~0.9 ms, the chunk walk). Typing undo/redo: 0.007-0.009 ms per step
either way (skipped by the attribute check).

Tests: `crates/loro/tests/undo_text_style_concurrent.rs`,
`crates/loro-internal/src/undo/text_styles.rs`, and
`crates/delta/tests/test_text_delta.rs`
(`transform_retain_attributes_like_map_writes`).
