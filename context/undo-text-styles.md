# Undo/Redo of Text Styles Next to Collaborators' Edits

Verified against code 2026-11-28.

## Rule

For each text the step's own style ops mark, per character and style key:

1. The winning style op (highest lamport, then peer) is one of the step's own
   ops: the key gets the value of the latest covering op the step did not
   write, or `null` if none is left. This overrides the transformed diff.
2. The winner is another peer's op: it is left alone (the key is removed from
   the step's attributes). This is the rule undo follows for every container
   kind it reconciles with collaborators' later writes.
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
state, before anything is applied. The texts to correct are the containers of
the `StyleStart` ops in the step's spans (`marked_texts`, `OpLog::iter_ops`),
not the texts whose diff has a styled retain: a collaborator may have deleted
everything the step marked except text they typed inside it, which leaves the
positional diff with nothing to unstyle (and `undo()` used to return `false`).
For each such text still reachable it reads
`RichtextState::undo_style_overrides` (`container/richtext/richtext_state.rs`),
which walks the text chunks with the style ranges
(`StyleValue::value_without`, `style_range_map.rs`), and rewrites the text's
retains, adding a text diff when the step had none. Inserts and deletes of the
step are kept as they are.

A step served by its recorded inverse ([undo-log.md](undo-log.md)) has no
style ops: marks are never recorded (`txn.rs` `local_op_inverse_diff` returns
`None` for them), so such steps skip the history read entirely.

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

`DeltaRope::transform` (`crates/delta`) still ignores retain attributes
(`FIXME: transform the attributes`). Undo does not rely on it; changing it is a
separate, public `loro-delta` change.

## Limits

- A text is only rewritten if one of the step's style ops is still in its
  state (`undo_style_overrides` returns `None` otherwise).
- "Another peer" is any peer but the step's. A collaborator's style op that
  covers a re-inserted copy but is older than the step also wins (rule 2).
- Whether a concurrent insert at a style boundary takes the style is decided by
  peer order (anchors are list elements, `tracker/crdt_rope.rs`), not by the
  expand setting; that is merge semantics and is unchanged. See
  loro-dev/loro#1009.

## Cost

A step with style ops reads its own ops once (`iter_ops`, history already
loaded by the checkouts such a step does), walks each marked text's style ranges
once to see if the step's ops are there, then its text chunks once:
O(ops + chunks + style ranges), on top of the checkouts that mark undo already
does.

Measured 2026-11-28 (release, one undo+redo pair of a 1000-char mark with a
concurrent remote insert, averaged over 40 steps, three runs, same binary with
the restyle switched off for comparison): 100k chars / 1k marks 0.83-1.11 ms
per step with, 0.73-1.13 ms without (noise); 1M chars / 10k marks 2.45-2.55 ms
with, 1.51-1.52 ms without (+~1 ms, the chunk walk). Typing undo/redo:
0.0075-0.0081 ms per step either way (recorded inverse, no history read).

Tests: `crates/loro/tests/undo_text_style_concurrent.rs`,
`crates/loro-internal/src/undo/text_styles.rs`, and
`crates/loro-wasm/tests/undo_text_styles.test.ts` (UTF-16 positions).
