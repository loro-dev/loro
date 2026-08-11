# Shallow Snapshot Dead-Style Redaction

Verified against code 2026-08-11

Shallow snapshot export nulls out the values of rich-text style pairs that are
"dead" at the shallow root — pairs whose anchors no longer enclose any text.
Without this, the value of a mark whose whole range was deleted before the
shallow root would ship verbatim in the exported bytes even though no read API
can reach it (issue/PR #1057; the shallow-snapshot docs advertise this export as
a content-redaction mechanism).

## Where

- Core transform: `redact_dead_style_values` in
  `crates/loro-internal/src/state/richtext_state.rs` (`mod snapshot`). It
  rewrites only the columnar tail of a Text container's state payload; the text
  prefix is borrowed, never copied.
- KV-level driver: `redact_dead_text_styles` in
  `crates/loro-internal/src/encoding/shallow_snapshot.rs`. It scans only the two
  Text key ranges of the state KV (container keys start with the container-type
  byte), so blocks holding other containers pass through in compressed form.
- Applied in every shallow export path before the KV is exported:
  both reuse branches and the checkout branch of
  `export_shallow_snapshot_inner`, and `export_state_only_snapshot`. The reuse
  branches re-run it so blobs produced before this change are cleaned on
  re-export.

## Invariants — read before changing any of this

1. **Only the value may be nulled; anchors must stay.** Text op positions are
   entity indexes and every style anchor counts as one element. Removing an
   anchor from the root state would shift the positions of every retained-tail
   and future op and corrupt replay.
2. **Pairs with expand type `Both` are never redacted.** An empty both-expand
   pair still captures future inserts (`find_best_insert_pos` walks past the
   start anchor and stops before the end anchor), so its value is live data:
   nulling it would make a shallow replica render differently from full-history
   replicas — a convergence violation. For After/None/Before, insertion can
   never land inside an empty pair, so the value is unobservable.
3. **Why "dead at the root" is permanent.** The only way text can appear inside
   an empty non-both pair is an insert concurrent with the range deletion. Such
   an op has deps before the shallow root and every import path rejects it via
   `preflight_import_changes` → `import_deps_before_shallow_root`
   (`ImportUpdatesThatDependsOnOutdatedVersion`). So within a shallow doc's
   importable universe the pair stays empty forever.
4. **The exported latest state uses the root's pair set as a whitelist.** When
   the export also carries the encoded latest state (`ops_num` above the
   threshold, or state-only mode), only pairs already redacted at the root are
   redacted there. Redacting pairs that die *after* the root would break
   historical checkout: a checkout into the retained range must still render
   those style values.
5. **Null-valued anchors are a pre-existing state shape.** Unmark creates them,
   decode accepts them, and the insert-position logic handles them
   (a null/false-valued start anchor blocks insertion before it), so the
   redacted state needs no decode-side changes. Side effect: on a redacted
   replica a local insert next to a dead expand-before pair lands before the
   start anchor instead of after the end anchor. Both positions are the same
   text position and ops carry absolute entity positions, so replicas still
   converge.
6. **Redaction runs after `remove_same`.** A latest-state entry that was
   byte-identical to the root entry dedups away and resolves to the root's
   redacted version on import, which matches what whitelist redaction would
   have produced.

## What still ships

- The style key, info flags, and anchor ids (needed for entity indexing and
  future expand behavior) — the same residual metadata as the JSON `redact` API
  from #504, which also nulls mark values rather than dropping ops.
- Dead both-expand pair values (see invariant 2).
- Anything in the retained op tail: marks created after the shallow root are
  ordinary history. With a multi-head frontier the root walks back to a common
  ancestor and the tail can contain far more than expected — the redaction
  promise only covers state before a single-head root.

## Tests

- Unit: `state::richtext_state::snapshot::tests` (pairing, whitelist,
  idempotency, malformed payloads, decode round-trip).
- Integration: `crates/loro/tests/integration_test/shallow_snapshot_test.rs` —
  the #1057 regression, the both-expand keep + convergence guard, root-alive
  style retention with historical checkout, latest-state-bytes and state-only
  paths, and per-expand-type convergence after typing at the collapsed range.
