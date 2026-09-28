# loro-js Rich-Text Anchors

Verified against code 2026-09-28.

loro.js keeps rich-text style anchors in the text sequence, as Rust does, so a
Text op position means the same thing in both runtimes. This article covers the
model, the position rules, and how the indexes keep the
[complexity contract](loro-js-performance.md).

## Why anchors are sequence elements

Rust writes a mark as two ops: `StyleStart { start, end, key, value, info }` at
counter `c` and `StyleEnd` at `c + 1`. Each inserts a zero-width anchor into the
Fugue sequence (`RichtextDiffCalculator::apply_crdt_op_to_tracker` in
`crates/loro-internal/src/diff_calc.rs`): the start anchor at entity position
`start`, then the end anchor at `end + 1`, in a causal view that now contains the
start anchor. Every Text op position (`insert.pos`, `delete.pos`,
`mark.start/end`, binary and JSON alike) is an *entity index*: Unicode scalars
plus visible anchors, in the op's causal view (`container/richtext.rs`).

Converting positions only when importing or exporting cannot be exact. A
concurrent insert can have an anchor as its Fugue origin, and whether text is
inside a style depends on its Fugue order relative to the anchors. So loro.js
stores the anchors in `LoroText._sequence`.

## Data model

- An anchor is a `TextElement` with `value: ""` and `anchor: { style, isEnd }`
  (`loro-js/src/runtime/containers.ts`). The two anchors of a mark share one
  `TextStyle`: `startId`, `lamport`, `info`, `value`, `key`, and the op's `end`.
  The start anchor has the `StyleStart` ID and lamport, the end anchor the next
  counter and lamport.
- Anchors use single-element storage. `TextSequenceSpan` never holds one;
  `LoroText.compact` keeps 32-element chunks with anchors as element arrays.
- `SequenceIndex` counts the elements with no UTF-16 width in each node
  (`ownZeroWidth`, `ownVisibleZeroWidth`, `visibleZeroWidth`, `allZeroWidth`).
  Lists never have one. A sequence without anchors pays one addition per
  recompute.
- Styles stay in `TextStyleIndex`, keyed by element ID, but membership now comes
  from the anchors: a style covers every element physically between its start
  and end anchors, both included. That includes hidden elements and elements
  inserted there later, and it never changes once the elements exist. A version
  only filters which styles apply.
- `LoroText` keeps each peer's start-anchor counters in a sorted array
  (`#anchorStarts`), so an ID run can be split at anchors without looking up its
  elements.

## Positions

| Unit | How loro.js converts it |
| --- | --- |
| UTF-16 / UTF-8 | The existing prefix metrics. Anchors have width 0, so every entity index in a run of anchors maps to the same offset. |
| Unicode | `entity - zeroWidthBeforeVisibleIndex(entity)`; back with `visibleIndexOfWidthElement(unicode)`. O(log n). |
| Entity | The visible index of `SequenceIndex`. Op positions use it. |

Local edits follow Rust's `TextHandler`:

- **Insert** (`LoroText.#insertPosition`, Rust `find_best_insert_pos`): start
  right after the scalar that ends at the UTF-16 position (or at 0), then skip
  the anchors there until the first one that must stay after the new text: a
  start anchor of an unmark (`null` or `false` value), a start anchor of a style
  that does not expand before, or an end anchor of a style that expands after.
  Rust's insert-position cache used to bypass this rule at text-leaf boundaries
  (fixed in loro-dev/loro#1135;
  `insert_position_near_style_anchors_does_not_depend_on_cursor_cache` in
  `crates/loro/tests/contracts/text_richtext_advanced.rs`).
- **Delete** (`LoroText._deleteRuns`, Rust `get_text_entity_ranges`): the text
  from the scalar containing the first UTF-16 unit to the one containing the
  last, as runs that are consecutive in both entity position and ID. Anchors are
  never deleted; they split runs. Ops are written last run first, so each
  recorded position is still valid when applied.
- **Mark** (`LoroText.mark`): both ends use the insert rule. Like Rust's
  `mark_with_txn`, the mark is skipped when every entity position in the range
  already resolves the key to the value, or when it is an unmark and no position
  has the key. `start >= end` throws, as in Rust. Detached texts insert anchors
  with Rust's `TextStyleInfoFlag::BOLD` info.
- **applyDelta** follows Rust's `apply_delta`: inserted text without an
  attribute explicitly drops each style it would inherit (the styles shared by
  both visible neighbors), and all marks run after the whole delta.

Remote ops:

- Text inserts and anchors use the same Fugue insertion at the entity position
  in the op's causal view.
- Deletes are resolved by position in the causal view, like Rust's tracker, not
  by `start_id` (`LoroText._deleteTargets`).
- A new element takes every style shared by its physical neighbors, which is
  exactly the set of styles whose anchors enclose it (`#inheritGapStyles`). A
  new end anchor applies its style to the physical range between its anchors
  (`LoroText._applyStyleEnd`).

## Styles, versions, events, and undo

- The value of a key is the covering style with the greatest `(lamport, peer)`;
  `null` means the attribute is absent (Rust `StyleValue::get`).
- A style applies once its end anchor is included in a version:
  `startId.counter + 1 < version` (`latestIncluded` in `text-style-index.ts`).
- A mark's event is emitted with its end anchor and covers the text whose
  resolved value changed (`LoroText._styleChangeRuns`).
- Checkout toggles anchors like inserted text and reports style changes through
  `TextStyleIndex.transitions` over the style's member runs. Anchors take the
  bulk path of `#applyVersionTransition`, which records no event, so the text's
  event baseline is started when an anchor op is collected; without it, a
  version with a start anchor but no end anchor was diffed against an empty
  text and reported the whole text as inserted.
- Undoing a mark gives back, wherever it changed the value, the value from just
  before its op (`LoroText._undoStyle`). That matches Rust for local histories;
  loro.js undo still does not transform against remote edits the way Rust's
  `UndoManager` does.

## Snapshots

`LoroDoc.#containerState` writes Rust's text state layout (`encode_snapshot_fast`
in `crates/loro-internal/src/state/richtext_state.rs`): text spans and anchors in
document order. A start anchor is a zero-length span followed by its mark entry;
an end anchor is a span of length -1 at the start counter + 1 that keeps the
start's lamport offset. Contiguous text is merged into one span.
`#hydrateContainerState` rebuilds the same elements and each style's range
(`LoroText._appendElements`). A Text loaded from a snapshot hydrates lazily;
every position lookup (`#entityFrom`, `#insertPosition`) hydrates first, because
the anchors decide where inserted text goes.

Shallow snapshots null the value of every style with no text between its anchors
at the shallow root, except both-expand styles, and null the same styles in the
latest state (`redactDeadStyleValues` in `document.ts`, Rust's
`redact_dead_style_values`; see
[shallow-snapshot-style-redaction.md](shallow-snapshot-style-redaction.md)).
Anchors and IDs stay, so positions do not move.

## Complexity

- Without anchors, every conversion short-circuits on
  `visibleZeroWidthLength === 0`, and delete runs use `visibleIdRuns` directly.
- Unicode conversions, the insert rule, delete runs, and applying a style are
  O(log n + anchors at the position + returned runs). Applying a style to a
  contiguous range is O(log n + ID runs), as before.
- Inserting inside styled text intersects two style histories per insert, which
  is proportional to the number of styles covering that position.

## Data written by loro.js 0.2

loro.js 0.2 wrote and read Text positions without the anchors. Decision
(2026-09-28): later versions read all data with Rust's positions and add no
version marker, like the plain-text changes in
[loro-js-rust-differential.md](loro-js-rust-differential.md). Reasons: a Text
op position must mean the same thing in both runtimes; documents shared with
`loro-crdt` peers had already diverged (each runtime read the other's ops at
other positions); and a marker would need an encoding change that Rust does
not have. The cost: in a document edited only with 0.2, every op after a mark
can apply elsewhere, and a 0.2 snapshot keeps its 0.2 state while its history
reads like Rust (a checkout there and back switches to Rust's reading). The
package version is major; `loro-js/README.md` ("Upgrading from 0.2") gives the
migration paths, and `loro-js/tests/legacy-data.test.ts` pins the readings of
0.2 fixtures in both runtimes.

## Testing

- `loro-js/tests/richtext-anchors.test.ts` compares loro.js with Rust fixtures
  (`loro-js/scripts/write-rust-richtext-fixtures.cjs`): the JSON ops loro.js
  writes, imports of Rust updates and snapshots, and checkouts.
- `crates/loro/tests/loro_js_interop.rs` imports loro.js rich-text updates and
  snapshots written by `loro-js/scripts/rewrite-rust-fixture.mjs`.
- The randomized Rust differential suite is described in
  [loro-js-rust-differential.md](loro-js-rust-differential.md).
