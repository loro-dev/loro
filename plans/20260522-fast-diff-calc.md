# Fast Diff Calc Tracker Span Routing Plan

Date: 2026-05-22

## Goal

Improve checkout diff calculation for documents with many peers and many text/list-like containers by avoiding repeated version-vector scanning and repeated empty `IdToCursor` lookups inside each container tracker.

The target direction is to move tracker checkout APIs away from "checkout to target `VersionVector`" and toward "apply these directed counter spans". The diff calculator should compute or route the relevant spans once, then pass only container-relevant spans into each tracker.

## Implementation Status

Implemented on branch `feat/scale-text-checkout-perf`:

- `c350b0e8 bench: add many text checkout scenario`
- `5c3cd62a refactor: route richtext checkout through spans`
- `91e5ceb6 perf: filter richtext checkout spans by coverage`

Current implementation covers:

- Phase 0 profiling counters for tracker spans, filtered spans, skipped spans, `IdToCursor::iter` calls, and empty `IdToCursor::iter` calls.
- Phase 1 directed richtext tracker span checkout API, with existing `checkout`, `checkout_causal`, and `diff` APIs kept as adapters.
- Removal of the tracker-only `current_frontier_hint`.
- Phase 2 per-container coverage filtering for text/list/movable-list richtext trackers, with conservative fallback when coverage is unavailable.
- Phase 3 filtered final diff materialization through coverage-aware tracker diff.

Benchmark notes for `multi-container/latest-to-base` with the default 1000 peers, 10000 changes, 10000 text containers, 8 large text containers, and `LORO_TEXT_CHECKOUT_PROFILE=1`:

| Version | Time | Avg total | Avg diff calc | Avg tracker checkout | Avg tracker diff |
| --- | ---: | ---: | ---: | ---: | ---: |
| `c350b0e8` baseline | 905.53-908.52 ms | 916.002244 ms | 900.339939 ms | 412.268949 ms | 424.615097 ms |
| `91e5ceb6` current | 811.54-879.43 ms | 842.044498 ms | 825.496539 ms | 476.979320 ms | 278.143902 ms |

Current profiling counters for the same run:

- `tracker_spans=377117000`
- `filtered_tracker_spans=123753500`
- `skipped_tracker_spans=253363500`
- `id_to_cursor_iters=123753500`
- `empty_id_to_cursor_iters=123623500`

This shows the routing is skipping about two thirds of tracker span checks in the target benchmark. The remaining empty iterator count is still high because the first implementation stores one broad coverage span per `(container, peer)`, which intentionally allows false positives. Phase 4/5 should only be considered if this remaining cost shows up in production profiles.

## Current Architecture

The current checkout diff flow is:

1. `OpLog::iter_from_lca_causally()` finds the LCA between `before` and `after`, computes the merged VV, and iterates changes causally from the LCA to the merged version.
2. `DiffCalculator::calc_diff_internal()` iterates each change/op and calls the per-container calculator.
3. Before applying the first op for a container in each change, the container calculator asks its tracker to checkout to the causal version immediately before that op.
4. `RichtextTracker::checkout_causal(CausalVersion)` computes `retreat` and `forward` spans internally by comparing its current VV against the target causal version.
5. `RichtextTracker::_checkout_spans()` applies the resulting spans by iterating `IdToCursor`.
6. At final diff materialization, `RichtextTracker::diff(from_vv, to_vv)` again does two full tracker checkouts: first to `from`, then to `to` with diff status enabled.

Important current details:

- `IdToCursor` is already internally keyed by `PeerID`: `FxHashMap<PeerID, Vec<Fragment>>`.
- Empty `IdToCursor::iter(span)` is cheap for a single call, but expensive when multiplied by many containers and many checkout steps.
- `CounterSpan` already has direction semantics. `start < end` is forward, `end < start` is reversed/retreat. `content_len()` uses absolute length, and `slice()` preserves direction.
- Existing `VersionVectorDiff` uses separate `retreat` and `forward` maps. Its internal `merge()` normalizes spans, so it should not be reused as-is for a single directed-span map API.
- `current_frontier_hint` is only maintained in the tracker today. It is not used as a fast path, and a single frontier hint is not enough to prove full causal equality.

## Main Performance Problem

The biggest waste is not just wide VV comparison. It is that a global peer span is handed to every affected text/list-like tracker even when that container has no op in that peer/counter range.

Example:

```text
global delta: peer 7, 0..1_000_000
containers: 1000 LoroText roots
container A has peer 7 ops
container B..Z have no peer 7 ops in that span
```

Without container routing, every tracker still checks `id_to_cursor.iter(peer 7, 0..1_000_000)`.

The proposed cache:

```rust
FxHashMap<ContainerIdx, FxHashMap<PeerID, CounterSpan>>
```

is a good first-order way to skip most of these empty checks.

## Critical Semantic Split

There are two similar but different structures. They should not be conflated.

### 1. Persistent Container Coverage Cache

This answers:

> Could this container possibly have any op from this peer in this counter range?

Recommended representation:

```rust
type ContainerPeerCoverage = FxHashMap<ContainerIdx, FxHashMap<PeerID, CounterSpan>>;
```

For coverage, spans should be treated as a coarse normalized min/max range. Direction is not meaningful because coverage is independent of checkout direction.

False positives are allowed:

```text
container has peer 1 ops at 10 and 1000
coverage stores 10..1001
query 500..600 falsely says "maybe"
```

False negatives are not allowed.

### 2. Per-Checkout Directed Delta

This answers:

> From this tracker's current visibility state to the target visibility state, which peer/counter spans should be forwarded or retreated?

Recommended representation:

```rust
type DirectedPeerSpans = FxHashMap<PeerID, CounterSpan>;
```

Here `CounterSpan` direction is meaningful:

```rust
CounterSpan::new(10, 20) // forward 10..20
CounterSpan::new(20, 10) // retreat 20..10
```

For a single transition, a given peer should only have one direction. If implementation ever needs both directions for the same peer, that means it is combining multiple transitions and must flush or split the delta.

## Proposed Design

### New Internal Types

Start with explicit names even if they are just type aliases initially:

```rust
type PeerSpanMap = FxHashMap<PeerID, CounterSpan>;

struct ContainerOpCoverage {
    by_container: FxHashMap<ContainerIdx, PeerSpanMap>,
}

struct TrackerCheckoutSpans {
    by_peer: PeerSpanMap,
}
```

`ContainerOpCoverage` stores broad normalized coverage. `TrackerCheckoutSpans` stores directed per-transition spans.

Do not encode checkout direction in a persistent coverage cache. Preserve direction only in `TrackerCheckoutSpans`.

### Span Operations Required

Add helpers instead of using `CounterSpan::get_intersection()` directly. The existing intersection helper assumes forward spans.

Needed helpers:

```rust
fn normalized_overlap(a: CounterSpan, b: CounterSpan) -> Option<(Counter, Counter)>;

fn intersect_preserve_direction(
    directed: CounterSpan,
    coverage: CounterSpan,
) -> Option<CounterSpan>;

fn extend_coverage(coverage: &mut CounterSpan, op_span: CounterSpan);

fn merge_directed_delta(existing: &mut CounterSpan, incoming: CounterSpan) -> Result<(), MixedDirection>;
```

Rules:

- Coverage should store normalized min/max ranges.
- Directed delta should preserve `start/end` direction.
- Intersecting a reversed directed span with coverage must return a reversed span.
- Merging directed deltas must reject mixed directions for the same peer in one transition.

### Tracker API

Add a new API:

```rust
impl Tracker {
    pub(crate) fn checkout_peer_spans(&mut self, spans: &PeerSpanMap);
}
```

Execution rules:

1. Iterate reversed spans first and run retreat logic.
2. Iterate forward spans second and run forward logic.
3. For `IdToCursor::iter`, use normalized spans internally.
4. Update `current_vv` using the directed span endpoint:
   - forward `10..20` sets peer end to `20`
   - retreat `20..10` sets peer end to `10`

Keep adapters temporarily:

```rust
checkout(&VersionVector)
checkout_causal(CausalVersion)
```

These adapters can compute directed spans and call `checkout_peer_spans()`. That keeps the first step behavior-preserving.

Remove `current_frontier_hint` after `checkout_peer_spans()` is in place. It is not a strong enough invariant and becomes unnecessary.

### Diff Calculator Routing

`DiffCalculator` should become responsible for deciding which spans are relevant to a container before calling the tracker.

For each container calculator, maintain or access `ContainerOpCoverage`.

When a global directed span is produced:

```rust
global: peer 7, 1000..2000
container coverage: peer 7, 1500..1600
directed for tracker: peer 7, 1500..1600
```

For retreat:

```rust
global: peer 7, 2000..1000
coverage: peer 7, 1500..1600
directed for tracker: peer 7, 1599..1499 or equivalent reversed slice
```

The exact reversed boundary helper must be tested carefully against `CounterSpan::contains`, `min`, `max`, and `norm_end`.

### Where Coverage Comes From

Preferred first implementation: per-diff-calculation, opportunistic coverage.

- When a container tracker applies an op, record that op's counter span into the coverage for that container.
- This coverage describes ops already known by that tracker.
- It is sufficient to filter most empty retreat/forward checks because tracker cannot act on op ids it has not seen yet anyway.

Special cases:

- Trackers seeded from shallow-root state chunks must also seed coverage for those chunks. Otherwise a later retreat over seeded content could be falsely skipped.
- Delete/move op counters must be recorded. The delete/move may refer to insert spans from other peers, but the version visibility toggle is caused by the delete/move op id itself.
- Style start/end ops must be recorded under their actual op ids, not only text insert ids.

Longer-term option: a persistent OpLog-level container op coverage index. This may be worth it if building coverage per diff calculation still costs too much, but it increases invalidation and shallow-history complexity.

## Design Risks

### Risk 1: False Negatives

False negatives in coverage are correctness bugs. They can leave tracker rope visibility wrong.

Mitigation:

- In debug/test builds, compare the new filtered spans against the old unfiltered checkout for selected cases.
- Add assertions that any `IdToCursor` entry affected by the old global span is included by the filtered span.

### Risk 2: Direction Loss

Existing code often normalizes spans:

- `IdToCursor::iter()` normalizes its input.
- `VersionVectorDiff::merge()` normalizes target spans.
- `IdSpan::ctr_start()` returns normalized start.

This is fine for lookup, but not for representing a transition. The new directed API must preserve direction until after it updates `current_vv`.

### Risk 3: Mixed Direction for Same Peer

For one transition from current to target, a peer cannot both advance and retreat. But if the implementation accumulates spans across multiple transitions before flushing, mixed direction can appear.

Mitigation:

- Scope `TrackerCheckoutSpans` to a single target checkout.
- Reject mixed direction in helper code.

### Risk 4: Final Diff Still Uses Full VV Checkout

`RichtextTracker::diff(from, to)` currently calls:

```rust
checkout(from)
checkout(to, on_diff_status = true)
```

If only `apply_change()` checkout is optimized, final diff materialization may still scan full VVs for every text container.

Plan must include `diff_by_spans(from_spans, to_spans)` or equivalent container-filtered final diff checkout.

### Risk 5: Sliced Changes and Partial Ops

`calc_diff_internal()` may slice ops when the replay range starts or ends in the middle of a change/op.

The filtered spans must align with the actual op slice being applied. It is acceptable for coverage to be broader than exact op slices, but it must never omit a sliced op that the tracker can see.

### Risk 6: Shallow Snapshot / Unknown Chunks

The current branch can seed richtext trackers from shallow-root state chunks. Coverage seeding must understand those chunks, or shallow checkout may skip spans that correspond to already-seeded tracker entries.

Unknown chunks and GC/shallow root fallback paths should remain conservative: if coverage cannot be proven, pass the original global span through.

## Phased Plan

### Phase 0: Measurement

Add test-utils profiling counters around tracker checkout:

- number of global checkout spans
- number of container-filtered spans
- number of spans skipped by coverage
- number of `IdToCursor::iter` calls
- number of empty `IdToCursor::iter` calls
- max/avg peers per checkout
- max/avg affected containers per checkout

Use current benchmarks that model many peers and many text roots. Keep before/after numbers in benchmark notes.

### Phase 1: Extract Tracker Span API

Implement:

```rust
Tracker::checkout_peer_spans(&PeerSpanMap)
```

Keep existing APIs as adapters:

- `checkout(&VersionVector)`
- `checkout_causal(CausalVersion)`
- `diff(from_vv, to_vv)`

No routing yet. This phase should be behavior-preserving.

Remove `current_frontier_hint` in this phase if no longer needed.

Verification:

- tracker unit tests
- existing richtext/list/movable-list diff tests
- `cargo test -p loro-internal checkout`
- focused fuzz artifacts that previously hit checkout/diff calc

### Phase 2: Add Container Coverage and Span Filtering

Introduce `ContainerOpCoverage` in diff calc or in each container calculator.

Start with per-diff-calculation coverage:

- record op spans when a tracker applies an op
- seed coverage when tracker is seeded from existing state chunks
- use coverage to filter global directed spans before calling `checkout_peer_spans`

Keep fallback conservative:

- if no coverage exists for a container, use current behavior
- if helper cannot safely preserve direction, use current behavior

Verification:

- debug comparison mode: run old unfiltered checkout and new filtered checkout on cloned trackers for small tests
- tests with many containers where only one container has ops in a wide peer span
- tests for reversed/retreat spans
- tests for sliced ops
- tests for delete/move/style ops

### Phase 3: Container-Filtered Final Diff

Add tracker diff API that accepts directed/container-filtered spans:

```rust
Tracker::diff_by_spans(from_delta, to_delta)
```

or split it into:

```rust
checkout_peer_spans(from_spans)
checkout_peer_spans_mark_diff(to_spans)
```

This avoids doing full `from_vv` and `to_vv` checkouts for every richtext tracker during final diff materialization.

Verification:

- compare final `InternalDiff::RichtextRaw` against old implementation
- include shallow-root seeded trackers
- include multi-frontier checkout

### Phase 4: Optimize Representation

Only after benchmarks show the map overhead matters, introduce inline variants:

```rust
enum PeerSpanSet {
    Empty,
    One(PeerID, CounterSpan),
    Small(SmallVec<[(PeerID, CounterSpan); 4]>),
    Map(FxHashMap<PeerID, CounterSpan>),
}
```

Do not start with this. It adds complexity before proving the basic routing wins.

### Phase 5: Optional Persistent Coverage Index

If per-diff coverage construction still costs too much, consider an OpLog/history-cache-level index:

```rust
ContainerIdx -> PeerID -> CounterSpan coverage
```

This must handle:

- import rollback
- shallow snapshot boundaries
- unknown containers
- history cache invalidation/freeing
- change-store compaction

Because of those lifecycle risks, keep it as a later optimization.

## Recommended First PR Scope

Do not implement the full cache in one PR.

First PR should:

1. Introduce directed span helpers with tests.
2. Add `Tracker::checkout_peer_spans`.
3. Make `checkout` and `checkout_causal` delegate to it.
4. Remove `current_frontier_hint`.
5. Add profiling counters for skipped/empty span checks, even if routing is not active yet.

Second PR should:

1. Add `ContainerOpCoverage`.
2. Filter checkout spans per container.
3. Keep a conservative fallback path.
4. Add correctness comparison tests.

This reduces blast radius and gives a clean place to benchmark API refactor vs container routing separately.

## Open Questions

1. Should coverage live in `DiffCalculator`, `RichtextDiffCalculator`, or a shared tracker layer?
2. How should shallow-root seeded richtext tracker coverage be initialized for style chunks?
3. Should movable-list use the same tracker span API immediately, or be migrated after richtext/list?
4. Is coarse one-span-per-container-peer enough for the known benchmarks, or do sparse same-peer histories require `SmallVec<CounterSpan>` later?
5. Should `VersionVectorDiff` be adapted to expose directed spans, or should this remain a separate type to avoid changing existing semantics?

## Current Recommendation

Proceed with the span-routing design, but keep two invariants explicit:

1. Persistent coverage is broad and directionless.
2. Per-checkout deltas are directed and scoped to a single transition.

This design is more general than `current_frontier_hint`, directly addresses the many-container empty-lookup cost, and can be introduced incrementally with conservative fallback paths.
