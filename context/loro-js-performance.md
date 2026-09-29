# loro-js Performance Architecture

Verified against code 2026-09-28.

The pure TypeScript runtime lives in `loro-js/src/runtime`. Its performance
target is the asymptotic behavior of the Rust runtime, while accepting a larger
JavaScript constant factor.

## Indexed state

- `sequence-index.ts` is the Text/List/MovableList order-statistic treap. A node
  stores up to 32 adjacent Unicode scalars or list items; scalar nodes stay
  light, while multi-element spans keep local visibility and encoding prefix
  indexes. Appending at a node's right boundary updates the span in O(1), while
  inserting inside a span moves at most 32 locations. Each subtree caches
  physical length, visible Unicode length, UTF-16 length, and UTF-8 length. It
  indexes operation IDs and historical insertion/deletion counter ranges
  incrementally; movable-list lamports are indexed on first query, then
  maintained incrementally. Sequential operation counters use dense arrays,
  distant counters use sparse range indexes, and randomly deleted target IDs
  use sorted 1,024-counter pages. Position, ID, cursor, and encoding-unit
  conversions are expected O(log n);
  materializing output remains O(output size). Elements store their node and
  bounded-span offset under module-scoped symbols, avoiding a separate
  WeakMap location object per scalar. Subtrees also cache whether their visible
  IDs form one consecutive run. Converting a visible range to delete/style ID
  runs, mapping an ID run back to UTF-16 event ranges, or obtaining ID runs from
  a historical causal view is therefore expected O(log n + returned runs)
  instead of O(characters) for contiguous text.
- Contiguous ID-span deletes store causal metadata as disjoint target/delete ID
  ranges. A physically contiguous subtree can be hidden with one lazy flag and
  cached-metric update instead of touching its descendants. Small fragmented
  spans use the operation-ID location index, recompute each touched 32-element
  span once, and recompute only the union of their ancestor paths; this avoids
  scanning a fragmented B4 tree for every small delete. A single-element delete
  keeps a smaller scalar fast path. Its operation counter is stored densely and
  its randomly ordered target ID is stored in a paged index, avoiding one
  balanced-tree node per B4 deletion. Scalar delete-counter indexes remain only
  for isolated and one-to-many deletes.
- `ordered-index.ts` is the ordered rank index used for map keys and tree
  children. Insert/delete/rank lookup are expected O(log n), while ordered
  iteration is O(n).
- Rich-text style anchors are zero-width elements of the Text sequence, as in
  Rust ([loro-js-richtext-anchors.md](loro-js-richtext-anchors.md)). Each
  subtree also counts its elements without UTF-16 width, so Unicode and entity
  positions convert in O(log n); a sequence without anchors takes the old fast
  paths.
- `text-style-index.ts` stores style histories in disjoint operation-ID ranges,
  separately from scalar Text elements. A style covers the elements physically
  between its anchors, so applying one is expected O(log n + ID runs in its
  range). Checking or undoing a style range is expected O(log style-runs +
  affected style-runs). Full-range marks and their subscribed checkout events
  no longer write or inspect every character. Delta and snapshot output reuse a
  run-local style resolver so their work remains linear in returned text and
  style runs.
- `LoroDoc` maintains per-peer change arrays, end counters, operation counts,
  current frontiers, sorted-history cache, and per-change dependency-version
  caches. Latest version/frontier lookup is O(peer/frontier count), and
  change-by-ID lookup is O(log changes-for-peer).
- Version ranges and explicit ID spans use the per-peer arrays to seek directly
  to the first overlapping change. A tail export or forward checkout is
  proportional to the selected changes, not all retained history. Incremental
  imports apply only newly integrated records.
- Retreat and comparable-version transitions toggle only the affected sequence
  elements, map keys, tree nodes, counters, text-style entries, and
  movable-list values. Map and Tree winner lookup uses per-subject/per-peer
  arrays with binary search. MovableList moves retain before/after neighbor
  anchors and an operation history per container. Direct switches between
  concurrent move branches replay only the affected container's order history,
  then apply the minimum move set selected by a longest-increasing-subsequence
  pass. Unrelated document history and container state are not rebuilt.
- Contiguous Text/List insertion and deletion transitions reuse the physical ID
  runs and reversible lazy subtree visibility in both directions. Without an
  event subscriber, hiding or showing one complete run is expected O(log n +
  touched physical runs). A subscribed restoration remains O(output size)
  because its event must contain the restored text or list values.
- Event snapshots are skipped entirely when a document has no event
  subscribers. With a subscriber, local transactions, incremental imports, and
  forward checkout compose Text/List deltas in an order-statistic piece treap;
  a small edit does not copy its whole sequence. Map, Counter, and Tree events
  likewise retain only the transaction-relative keys, value, or nodes needed to
  produce the final event.
- A pending transaction stores its accumulated operation length and causal
  version incrementally. Never recover either by reducing all pending ops.
- Plain Text/List elements allocate delete, value, and move metadata only when
  an operation needs it; Text style metadata lives in the range index. A
  multi-scalar Text insertion stores its string and UTF-16 boundaries once in a
  `TextRunBuffer`. Physical spans retain only buffer ranges, so splitting a
  piece does not copy its text or materialize its ID columns. Scalar views are
  created only when an API asks for one. Single-scalar edits retain the smaller
  object path because the B4 trace consists entirely of single-scalar inserts
  and constructing a temporary packed span for every edit costs more than it
  saves. `LoroText.compact()` is the explicit safe point for rebuilding heavily
  fragmented scalar storage as adjacent 32-element spans without changing
  history. Text iteration stops directly in the index when its callback returns
  `false`; `toString`, `slice`, and `iter` consume contiguous visible storage
  ranges and read a whole text-buffer range at once instead of allocating a
  scalar view and substring for every character. Range predicates can also stop
  inside the index; `Text.unmark` therefore does not materialize the inspected
  range before applying the mark operation.
- Text line metadata is optional. The first `lineCount`, `lineStart`, `lineAt`,
  or `getLine` query builds sparse per-buffer and per-node line-break offsets plus
  subtree totals. Splits share the buffer offsets rather than copying them. The
  node totals live in a sidecar so unopened line APIs do not enlarge the hot
  treap-node shape. Later edits maintain the index, and line/position lookup is
  expected O(log n). A line break is LF; `getLine` removes the preceding CR for
  CRLF input. Positions remain UTF-16 offsets.
- Text and List Fugue insertion use an incremental `originLeft` direct-child
  index only when a concurrent/future interval needs ordering. Consecutive IDs
  keep their single-child edge implicit, and `SequenceIndex` can skip an entire
  future ID run while finding the next causally included element. Ordinary local
  edits keep the smaller unindexed path. MovableList continues to use the scan
  because moves break the origin-tree physical preorder. Sibling subtrees are
  contiguous, so the gap between two direct children belongs to the earlier
  child. After the last child the interval can also hold concurrent elements
  whose origin is left of `originLeft`; Rust's scan stops before them. The index
  therefore checks whether the interval's last element descends from
  `originLeft` and otherwise binary-searches the boundary. The descent test
  walks origin-left links but jumps over each implicit run through per-peer
  sorted counters of explicit (non-consecutive) elements, so it costs
  O(explicit links · log n), like Rust's span-based scan, instead of one probe
  per scalar in a long concurrent run (loro-dev/loro#1139). A walk over the
  physical ID runs of the last sibling's subtree finds the same boundary in
  O((runs + 1) log n) and was measured against it (September 28, Node 22,
  1-minute load 8–17): with the B4 trace as the sibling subtree the binary
  search takes 7.3 ms versus 11.1 ms, but on a deep chain of explicit links
  (two positions typed alternately, 64k elements) 60.6 ms versus 15.5 ms,
  because each of its O(log n) probes walks the chain. Realistic traces favor
  the binary search. With a warm origin index, importing one concurrent
  character after a 512k-character typed run takes 0.28–0.31 ms (0.29–0.30 ms
  on `main`, which misorders other cases; a per-scalar walk took 9.7–10.7 ms),
  and `text-concurrent-insert-after-long-run` stays at 0.18–0.30 ms from 64k to
  512k characters (September 28, Node 22, interleaved, 1-minute load 7–17).
- An imported Text delete is resolved by its position in the op's causal view,
  like Rust's tracker (`LoroText._deleteTargets`): O(log n + runs) through
  `visibleIdRuns` or the cached causal view. The recorded `start_id` is only a
  fallback, because Rust's WASM build can record one that is off by the UTF-16
  length of astral text. Local deletes skip the lookup.
- Merging adjacent changes appends only the new operations and key-table entries
  to the retained record. The cached operation length, peer end, frontier set,
  operation indexes, and subscriber update slice are updated incrementally, so
  a stream of mergeable commits does not repeatedly copy or reduce its complete
  history. Consecutive List/MovableList inserts in one transaction also share a
  single operation value array.
- Snapshot SSTables choose interoperable LZ4 blocks when they reduce size.
  DeltaRLE state columns encode and decode as streams rather than allocating
  million-item BigInt intermediates, and LZ4 decode writes into typed storage.
  Importing an initial latest-state snapshot validates every state entry and
  frontier block immediately, but retains current state as an owned encoded
  SSTable. Root containers are hydrated at import; referenced descendants are
  decoded one SSTable block at a time when first accessed. Untouched blocks are
  copied directly during snapshot export, while dirty container entries are
  locally rewritten. The encoded history remains a read-only base and later
  local or imported changes use a small materialized overlay. Local edits, full
  update export, latest snapshot export, current reads, version, frontiers, and
  operation count therefore do not build the complete history DAG. Historical
  queries, checkout, partial-range export, and other APIs requiring arbitrary
  dependency traversal still build and validate all history indexes once on a
  staging document before installing them. Import subscribers retain eager
  state hydration because their import event must describe every changed
  container.
- Text, List, and MovableList state hydrated from a snapshot (eager, lazy, or a
  shallow root) has no tombstones, deletion index, or style, value, and move
  history for the snapshot's operations. `LoroDoc.#snapshotSequences` records
  each such container with its snapshot version. Before `checkout`,
  `checkoutToLatest`, `diff`, or a detached snapshot export
  (`#encodeLatestState`) transitions, `#prepareSnapshotTransition` looks only
  at the containers the transition
  touches: it hydrates lazily encoded ones (an untouched lazy container still
  holds its latest state, which is its state at the current version) and, when
  the transition crosses a snapshot operation, rebuilds that one container from
  its own operations (`#completeSnapshotSequence`, from its shallow root entry
  in a shallow document). Operations applied after hydration are indexed like
  any others, and Map, Tree, and Counter state needs no rebuild. The per-container
  record index is built once per history revision. Consecutive text inserts that
  continue each other replay as one span (`coalescedTextInsert`). Unrelated
  containers are never replayed and the lazy SSTable is kept, so snapshot export
  copies every untouched entry and rewrites only the touched ones. A delete
  transition is still refused unless the deletion index recorded that delete,
  for example one imported while detached.
- The completion compares the replay of a Text or List with the snapshot state
  (visible ids, plus the values). With style anchors in the sequence
  (loro-dev/loro#1137) a replay of Rust or loro.js history, styled or not,
  equals its snapshot state; before, loro.js did not count the anchors and a
  replay of Rust-created styled text could differ. A replay now differs only
  when the snapshot state disagrees with its own history: a snapshot written by
  loro.js 0.2 (see `loro-js/README.md`, "Upgrading from 0.2"), or a loro.js
  shallow snapshot with the retained-range gap described in
  [loro-js-rust-differential.md](loro-js-rust-differential.md). When they
  differ, the replay is discarded and the container becomes `unreplayable`: it keeps its snapshot
  state, encoded once, and is never given a replay. A transition that touches
  it runs without its operations and then moves it separately
  (`#planSnapshotStates`): when the installed state already has every forward
  operation (tracked as `applied`), a Text is toggled by id and style version,
  O(delta) as for a state without history; otherwise (for example an update
  imported while detached) it is rebuilt from the snapshot state plus the later
  operations the target includes (`#rebuildFromSnapshotState`, O(container
  size + its operations)). Events come from the transition's recording, or from
  whole-container values when style operations are crossed, since their ranges
  come from positions.
- What that guarantees: the latest state, imports, and exports equal the
  snapshot state plus the later operations, as loro.js applies them. A later
  operation concurrent with a delete that the snapshot already applied can
  still land at another position than in Rust, as on main (loro-dev/loro#1163).
  An older version is approximate: the snapshot state cannot restore text
  deleted before it. In the round-3 review's random Rust histories, older
  versions and `revertTo` differed from Rust more often than on main (437 vs
  317 checked versions, 287 vs 151 reverts), while main corrupted the latest
  state after a checkout round trip in 57 of 90 seeds and the PR in none.
  The anchor model (loro-dev/loro#1137) makes such text replayable and removes
  both costs. Two more gaps are also on main: after an update imported while
  detached, a shallow export on the live document can change its latest state
  (plain text too; loro-dev/loro#1136 fixes the plain-text case), and a shallow
  export that throws midway leaves the live document at the root or in
  between, since `#encodeShallowSnapshot` rebuilds it without a restore.
- A MovableList is not a snapshot sequence at all (`#markSnapshotSequence`);
  it behaves as on main. Its snapshot state names each element by its Rust
  position id (`#hydrateContainerState` takes `listItemIds` in order, and after
  a move they include invisible positions), while a replay names elements by
  their insert ids, and Rust encodes element ids as (peer, lamport), so no
  comparison with a replay is meaningful. The hydrated state also has no move
  or value history (`_moveHistoryComplete`, `_valueHistoryComplete`), so
  `#canTransitionRecords` refuses to cross its snapshot moves, sets, and
  deletes, and the transition replays to the target; later transitions are
  incremental, and a later move or set by element id resolves. Replaying the
  list to the current version and then retreating instead (as round 3 did)
  carried loro.js's MovableList non-convergence at the latest version into
  older versions. Completing only lists whose snapshot names elements by insert
  ids was also tried: it makes a snapshot document transition like a
  full-history one, and loro.js's incremental MovableList transitions have
  their own bugs with concurrent moves and sets (a full-history document on
  main shows them too), so random Rust histories got 167 mismatching versions
  against main's 91; leaving every MovableList on main's path gives 56. Until
  the MovableList model work (loro-dev/loro#1132 and follow-ups) hydrates
  element ids, a MovableList whose loro.js replay differs from Rust can change
  its latest state at the first such transition, also as on main. Once the
  list is replayed, later transitions are incremental and can still show a
  wrong older version where main, which replays on every such checkout, is
  right (round-4 review, seed 22: after checking out v11, v12, then v13, the
  list shows `[11]` where Rust, main, and a full-history document show `[9]`;
  the latest state and `revertTo` are right). This is loro.js's MovableList
  transition gap, left to loro-dev/loro#1132.
- A full `#rebuildFromHistory` (the non-incremental fallback, shallow export,
  `forkAt`) rebuilds unreplayable containers the same way. It no longer checks
  snapshot-hydrated styled Text first (`#checkSnapshotSequences`, removed in
  loro-dev/loro#1137): that extra replay per styled Text existed because the
  anchors shifted Rust positions, which the anchor model now counts. So styled
  and plain Text behave alike: a hydrated container that no transition has
  completed takes the replay of its history, which for a 0.2 snapshot is Rust's
  reading. Only the root state of a shallow export uses the replay, since the
  snapshot state is later than the root. `forkAt` keeps a snapshot state only
  in a fork whose version includes that state's version; an older fork has
  none of the operations needed to undo later ones in that state, so it keeps
  the replay of its own history, as on main, and stays consistent with its own
  operations. `tests/snapshot-checkout.test.ts` checks random checkouts,
  detaches, and imports on Rust rich-text histories (`rich-text-history.json`)
  against a document that only imports, plus forks, Rust MovableList moves
  (`movable-moves.json`), and a Rust text whose marked characters were deleted
  (`deleted-mark.json`).
- Transitions deduplicate sequence elements by id (`SequenceElementSet`): a
  packed Text span returns a new wrapper per lookup, so two concurrent deletes
  of one character used to delete it twice. A completion that throws
  reinstalls the snapshot state and leaves the container hydrated. A checkout
  that throws restores its previous version and state (`#transitionTo`, which
  also prepares inside its `try`), and `diff` restores the current state with a
  full rebuild when it or its move back throws.
- Before a transition, `#canTransitionRecords` checks that each sequence still
  holds the elements that the crossed insert operations name. It collects the
  runs per container and calls `containsIdRuns` once per container, reading IDs
  without building element views: O(elements + runs log runs). One call per
  operation made a checkout O(operations × elements): across 2k scattered
  inserts in an 8k text it took 698 ms on `main` and takes 4.2 ms now
  (`text-scattered-edits-checkout`, 13 → 698 ms from 1k to 8k before, 1.0 →
  4.2 ms now; September 29, Node 22, 1-minute load 6–9).
- First checkout after importing a 262,144-operation single-peer Text snapshot
  takes about 57 ms (medians of 5 alternating runs on a loaded Apple M5 Pro),
  versus about 148 ms for the earlier whole-document replay; 65,536 operations
  take 25 versus 43 ms, and a subscriber adds nothing measurable (earlier 192
  ms at 262,144). The replay of that one container dominates. Later checkouts
  stay around 0.1–0.4 ms. A doc with 32,768 child Maps that retreats one of them
  needs no replay: 57–61 ms versus 152 ms, with the same 234 MiB peak RSS as
  before the fix (earlier 284 MiB). The coalesced inserts also make importing
  the B4 trace as one update about 30% faster (about 195 versus 275 ms).
- A shallow history trims the ops that wrote root-time Map values and Tree
  placements (the root commit's other ops). When a Map or Tree retreat finds no
  retained winner at or below the target, it uses the shallow root state entry
  (`#shallowRootMapRecord`, `#shallowRootTreeNode`) instead of dropping the key
  or node; a retained Tree delete whose placement was trimmed takes the root
  placement and stays deleted. The root store entry for a container comes from
  `#shallowRootEntryIndex`: the key index the import builds to merge the root
  and latest states, or while hydrating the root store for a replay (Rust omits
  the latest state for a short retained tail). Each container's key/node index
  is built on first lookup. A retreat therefore touches only the maps and trees
  it changes, as in Rust's per-map checkout index seeding (loro-dev/loro#1120,
  #1124): the first such retreat in a shallow doc with 32,768 child Maps takes
  under 1 ms for both loro.js- and Rust-written snapshots, flat from 1,024
  Maps, and later ones about 0.02–0.03 ms.

When an element's deleted flag, tree parent/position, or map visibility changes,
mutate it through its owning index helper. Direct mutation leaves subtree or
ordered-key caches stale.

## Benchmarks

Build and run the complete Automerge-paper B4 trace:

```sh
pnpm --dir loro-js bench:b4 -- 259778 7
```

Measure text storage, reads, line lookup, explicit compaction, and retained heap
with:

```sh
pnpm --dir loro-js bench:text-buffer -- 131072 50000 7
```

In a same-machine Node 22.23.1 A/B against `origin/main`, the July 22 text
benchmark measured bulk 131,072-scalar insertion at 21.8 ms versus 33.2 ms,
`toString` at 0.40 ms versus 6.34 ms, a middle-half `slice` at 0.22 ms versus
3.67 ms, and `iter` at 2.00 ms versus 8.05 ms. Retained heap for the bulk
document fell from 22.9 MB to 10.7 MB. A separate alternating scalar-only probe
measured 31.0 ms versus 30.8 ms, a 0.8% difference within noise. Building the
optional line index took about 16.0 ms and retained about 1.46 MB; 1,000 indexed
middle-line lookups took about 0.71 ms versus 103.8 ms for repeated flat-string
scans. Explicitly compacting 50,000 maximally fragmented middle inserts took
about 20.6 ms and reduced physical nodes from 50,000 to 1,563. Six fresh-process
B4 pairs with alternating run order measured 234.4 ms at `origin/main` and 234.8
ms with the text changes, a 0.2% difference within run-to-run noise. The main
ESM bundle grew from 461.87 kB / 88.41 kB gzip to 485.79 kB / 92.11 kB gzip.

The script fully warms the largest requested prefix and releases the previous
sample before forced GC. It reports local editing plus snapshot/update import
and export. Run scaling probes for the main indexed structures and history APIs
with:

```sh
pnpm --dir loro-js bench:complexity -- 1000,2000,4000,8000
```

Measure a real latest-state snapshot through import, a local Map edit, a remote
update import, full update export, and snapshot export with:

```sh
pnpm --dir loro-js bench:snapshot-memory -- /path/to/document.snapshot
```

For the 11,387,982-byte test document with 423,797 operations and 115,147
containers, Node 26.4.0 reports 70.92 MiB RSS after loading the input and a
160.70 MiB process peak after snapshot export: an 89.78 MiB incremental peak.
Used JS heap peaks at 8.58 MiB. Snapshot import takes about 0.90 seconds, the
local commit about 1.9 ms, full update export about 6.6 ms, and snapshot export
about 57 ms on the measured Apple M5 Pro. Before lazy state and history-overlay
integration, the same workflow retained roughly 703 MiB heap immediately after
import, exceeded 860 MiB after the first local edit, and reached roughly 1.66 GB
RSS during snapshot export.

The ordinary fully materialized snapshot path remains neutral in a same-machine
A/B check. After three warmups, two 15-sample B4 snapshot-export runs measured
110.5/107.0 ms medians at the parent revision and 107.7/107.7 ms with lazy
snapshots. Both revisions emitted the same 309,780-byte snapshot.

On an Apple M5 Pro with Node 26.4.0, the complete 259,778-action B4 trace now
applies in a 353.5 ms three-sample median (351.8–354.9 ms samples) and finishes
at 104,852 UTF-16 code units. The resulting process reported 107.1 MB of used JS
heap and 322.1 MB RSS.
The original array implementation was estimated at 30–50 minutes. Prefix
measurements from 20k through the full trace scale approximately linearly. The
matching Rust Criterion benchmark has a 47.711 ms point estimate on the same
machine, so TypeScript is about 7.4x slower in absolute time. Merging
consecutive inserts of a transaction into one op, as Rust does, shrank the B4
update export from 1,153,540 to 274,574 bytes, and merging contiguous text in
the Text state shrank the snapshot from 309,780 to 206,553 bytes. B4 leaves 182,315
scalar objects but packs them into 13,613 TypeScript treap nodes. The same run
measured snapshot
export at 162.4 ms, update export at 129.3 ms, snapshot import at 161.5 ms, and
update import at 328.1 ms.

The `zxch3n/crdt-benchmarks` adapters provide a separate end-to-end comparison
against the published Loro WASM adapter. With the local `loro-js` build, B4 fell
from more than 180 seconds before the fixes (141.5 seconds after removing the
first copy path) to 2.846 seconds after incrementally maintaining merged-change
lengths; the WASM adapter result is 4.733 seconds. B3.5 takes 288 ms versus
303 ms. B3.3 emits a 240,032-byte snapshot versus roughly 242 KB from WASM,
down from the former 7.95 MB uncompressed TypeScript snapshot. A 60k-item List
update is 231,840 bytes and a 120k-character Text update is 120,095 bytes, both
matching the WASM output sizes. C1.1 still takes 6.988 seconds versus 1.728
seconds; its remaining gap is described below rather than treated as an
asymptotic regression.

With 1k through 64k retained changes, exporting, importing, or checking out only
the last change stays below 0.7 ms after warmup. Explicit one-operation span
exports stay below 0.3 ms.

A historical sequence view now counts whole physical ID subtrees as fully
included or excluded before descending. At 64k elements, cold views excluding
the full run, only the final element, or the suffix after one third take about
0.31, 0.34, and 0.11 ms respectively; the 1k through 8k warmed matrix keeps the
full-exclusion case around 0.01–0.06 ms. The eight most recently used causal
versions retain their computed views until the sequence changes; 1,000
alternating cached queries stay below 0.7 ms through 64k elements.

A subscribed one-character edit in a 64k Text is about 0.35 ms after
operation-composed event deltas, down from 34.6 ms when event generation copied
the whole Text. A transaction containing 64k subscribed middle inserts takes
about 294 ms in total and scales approximately linearly with the operation
count. Retreating or restoring a one-change tail, with or without a subscriber,
stays around 0.05–0.69 ms from 1k through 64k retained changes, including a
one-character mark, a MovableList set/move suffix, and a one-change `diff`.
Switching a four-element MovableList directly between concurrent move branches
stays below 0.4 ms while unrelated retained history grows from 1k to 64k
changes; the subscribed path stays below 0.6 ms. Switching branches that mix
move, insert, and delete operations stays below 0.5 ms, with or without a
subscriber, while unrelated retained history grows from 1k to 8k changes.
Deleting a contiguous 64k Text ID span takes about 0.5–0.8 ms, including the
subscribed path, versus about 101 ms for the scalar reference path in the latest
isolated run. The 1k through 64k
measurements remain nearly flat because the delete covers one physical ID run.
A cold causal view that excludes only the final element
stays below 0.5 ms at 64k sparse counters because its counter index is already
maintained.
On a detached 64k Text, stopping `iter` after its first chunk takes about 0.25
ms. Full `toString` takes about 1.5–1.6 ms versus 2.2 ms for the former
two-array path; slicing the middle 32k characters takes about 0.42 ms versus
1.27 ms for the former range-array path.

Applying a mark to one contiguous 64k Text run takes about 0.37–0.74 ms in an
isolated repeated probe. Retreating/restoring that full-range mark takes about
0.11/0.10 ms, and the subscribed restore takes about 0.41 ms. These operations
now scale with ID/style runs and emitted formatting ranges rather than the 64k
characters.

With style anchors in the sequence (September 28, Node 22, best of 7 on a
loaded machine, same process for both revisions): the 64k full-range mark takes
0.07–0.08 ms (0.06–0.08 before), its retreat/restore 0.06–0.07 ms (about 0.01
before), and the subscribed restore 0.06 ms. Typing 1,000 characters inside the
bold range takes 3.1–3.3 ms (1.7–2.1 before): each insert intersects the style
memberships of its two physical neighbors. Marking the same range again and
again nests anchors: the n-th mark's range contains the n-1 earlier start
anchors as separate ID runs, so applying it and moving the version across it
are O(n), as in Rust, whose `StyleRangeMap` has one segment per anchor there.
`text-repeated-mark-tail-{retreat,restore}` therefore grows with its size
(0.5/1.1/2.2/6.9 ms at 1k/2k/4k/8k, versus 0.16–0.29 ms before); the Rust WASM
build takes 7.1/19/362 ms to retreat 1k/4k/16k such marks, and building the 16k
history takes 402 s in loro.js and 542 s in Rust. Every other
`bench:complexity` entry stays flat from 1k to 8k.

The zero-width counters are four more fields in every treap node, maintained
on every update even for text without anchors, so plain Text edits pay a
constant cost: 8,000 subscribed middle inserts take 15.4–15.8 ms against
14.3–14.4 ms without them, and `text-subscribed-batch`, `history-commit`, and
`history-update-batch-import` are 12–20% slower at 8k (September 29, Node 22,
alternating runs at 1-minute load 6–8). No entry grows with size because of
them. Moving them into a sidecar that only anchored sequences allocate, like
the line-break totals, would remove that cost.

Reading a Rust-written styled Text from a snapshot (16k characters, 200 marks,
2k later edits) and checking out a middle version takes 24.7–25.0 ms the
first time, 4.1–4.3 ms back to the latest, and 11.8–12.2 ms for a fork; `main`
takes 35.9–37.2, 3.1–3.3, and 29.8–30.5 ms but shows wrong text, because it
marks the text unreplayable and toggles its snapshot state.

These costs were reviewed and accepted (loro-dev/loro#1137). The review measured,
on Node 26 at 1-minute load 25–40, typing 1,000 characters inside a 64k bold
range at 2.9–4.4 ms (1.9 ms on `main`) and the 8k repeated-mark tail retreat and
restore at 8.0–8.1 and 11.6–13.2 ms (0.4–0.6 ms on `main`). Re-measured after the
review fixes (two runs of two interleaved rounds with `main`, Node 22, 1-minute
load 5–25): the repeated-mark tail retreat takes 0.51–0.71/1.18–1.27/2.25–2.80/
6.11–8.25 ms and its restore 0.47–0.53/1.16–1.35/2.08–2.57/5.89–7.75 ms at
1k/2k/4k/8k (0.08–0.43 ms on `main`); a 64k full-range mark applies in 0.11–0.17
ms (0.06–0.12), retreats in 0.22–0.35 ms (0.11–0.14), and restores in 0.11–0.14
ms (0.02–0.03); typing 1,000 characters inside it varies with load (2.4–6.6 ms,
2.6–4.3 ms on `main` in the same alternating runs).

A subscribed forward checkout that combines a full-range delete and mark takes
0.41 ms at 1k characters and 0.16 ms at 8k after warmup. Historical mark
positions are converted directly to causal ID runs, and removed ID runs are
subtracted before event generation; the compact delete event no longer causes
an intermediate scan of every character.

Retreating and restoring a contiguous 64k insertion without a subscriber take
about 0.19/0.14 ms. Retreating and reapplying a contiguous 64k deletion take
about 0.4–3.2/0.13 ms across isolated runs. Subscribed transitions that only
emit a delete stay below 0.4 ms; restoring 64k values takes about 70–76 ms and
is proportional to the required event payload. A warmed 100-commit probe with
1k, 8k, and 64k unrelated container subscribers measures 0.018, 0.011, and
0.009 ms per affected-container commit, so dispatch does not scan unrelated
listeners.

The 1k, 2k, 4k, and 8k matrix also verifies that point/rank lookups, cursor
lookups through deleted gaps, cached causal views, map/root/tree path lookup,
unrelated-container subscriber dispatch, one-change sequence and style version
switches, one-change history import/export/checkout/diff, concurrent
MovableList branch switches, and 1,000 container-ID lookups do not grow with
unrelated retained state. Output-producing APIs such as `toJSON`, `toString`,
`getAllChanges`, snapshots, and full-version conversion remain proportional to
their returned or encoded data.

## Remaining constant-factor and memory gaps

The audited public paths have no known time-complexity gap from the Rust
runtime. Forward, retreat, and comparable-version checkout apply only their
version delta; contiguous insert/delete/style transitions use ID runs and lazy
subtree visibility. Compact subscribed transitions do not expand those runs.
Operations that return, encode, decode, or emit `n` values remain O(n), as in
Rust.

The remaining differences are representation and JavaScript constant factors:

- Multi-scalar Text operations use shared string/ID spans, but B4 inserts all
  182,315 scalars as separate operations. Its scalar fast path therefore still
  retains one object per inserted scalar until an explicit `compact()`. Bounded
  physical nodes cut B4's treap-node count by about 13.4x; inline locations also
  remove one location object and WeakMap entry per scalar. A direct
  scalar-to-packed-span mutation path was measured and rejected: Fugue ordering
  immediately reads IDs and origins, so temporary packed views slowed B4 more
  than they saved. Closing the remaining Rust memory gap needs raw column access
  in Fugue or compaction at a caller-chosen quiet point, not packing every edit
  unconditionally.
- A subscribed restoration of a large insertion or deletion must include the
  restored text/list values in its event, so its work is proportional to that
  emitted output. Without a subscriber, both hide and show transitions use the
  reversible lazy visibility layer and stay proportional to affected ID runs.
- Importing interleaved concurrent MovableList moves can canonicalize the
  affected container once. Initial snapshot hydration and fallback transitions
  with incomplete history can likewise materialize complete touched containers
  when their returned state or subscriber event requires it.
- The million-operation C1.1 concurrent-text trace still exposes a large
  constant-factor and retained-memory gap. Its local edit phase is about 4x the
  WASM adapter, and parsing the 6.5 MB snapshot takes 3.62 seconds versus 43 ms.
  Streaming DeltaRLE, typed LZ4 decode, and deferred history integration removed
  the known superlinear and temporary-allocation failures; closing the remaining
  gap needs a more compact decoded operation/frontier representation rather than
  another public-API complexity change.
- Old or manually constructed containers without a parent-edge binding scan
  their parent once, then cache the recovered binding. Normal container path
  lookup uses the indexed binding directly.

Keep randomized index-invariant coverage in `loro-js/tests/indexes.test.ts` and
Rust/TypeScript fixture coverage in `loro-js/tests/rust-interop.test.ts` when
changing these structures. The randomized Rust differential suite
(`loro-js/tests/differential/`, see
[loro-js-movable-list.md](loro-js-movable-list.md)) checks convergence, events and
encoding interchange against a WASM build of the Rust implementation.
