# loro-js Performance Architecture

Verified against code 2026-07-18.

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
- `text-style-index.ts` stores style histories in disjoint operation-ID ranges,
  separately from scalar Text elements. Applying, unapplying, or checking a
  style range is expected O(log style-runs + affected style-runs). Full-range
  marks and their subscribed checkout events no longer write or inspect every
  character. Delta and snapshot output reuse a run-local style resolver so
  their work remains linear in returned text and style runs.
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
  multi-scalar Text insertion is stored as a compact string/ID span and creates
  scalar views only when an API needs them. Single-scalar edits retain the
  smaller object path because the B4 trace consists entirely of single-scalar
  inserts and constructing a temporary span for every edit costs more than it
  saves. Text iteration stops directly in the index when its callback
  returns `false`; `toString` and `slice` traverse visible ranges directly and
  join bounded 32-character chunks instead of allocating element and character
  arrays for the full output. Range predicates can also stop inside the index;
  `Text.unmark` therefore does not materialize the inspected range before
  applying the mark operation.
- Text and List Fugue insertion use an incremental `originLeft` direct-child
  index only when a concurrent/future interval needs ordering. Consecutive IDs
  keep their single-child edge implicit, and `SequenceIndex` can skip an entire
  future ID run while finding the next causally included element. Ordinary local
  edits keep the smaller unindexed path. MovableList continues to use the scan
  because moves break the origin-tree physical preorder.
- Merging adjacent changes appends only the new operations and key-table entries
  to the retained record. The cached operation length, peer end, frontier set,
  operation indexes, and subscriber update slice are updated incrementally, so
  a stream of mergeable commits does not repeatedly copy or reduce its complete
  history. Consecutive List/MovableList inserts in one transaction also share a
  single operation value array.
- Snapshot SSTables choose interoperable LZ4 blocks when they reduce size.
  DeltaRLE state columns encode and decode as streams rather than allocating
  million-item BigInt intermediates, and LZ4 decode writes into typed storage.
  Text snapshot export streams visible elements and coalesces adjacent IDs with
  the same peer and lamport offset into one state span. Initial hydration
  validates every positive Text ID range before mutating the document, appends
  bounded 32-scalar spans without scalar ID objects, and reserves dense
  per-peer counter storage only for sufficiently compact ranges under a
  document-wide allocation cap.
  Importing an initial latest-state snapshot hydrates current containers and
  validates its frontier blocks immediately, while retaining the other owned
  history blocks in encoded form. Frontier validation discards its decoded
  operation objects instead of retaining a second full history graph. A history
  query, edit, checkout, export, or later update builds and validates all
  history indexes once on a staging document before installing them; current
  reads, version, frontiers, and operation count do not force that work.

When an element's deleted flag, tree parent/position, or map visibility changes,
mutate it through its owning index helper. Direct mutation leaves subtree or
ordered-key caches stale.

## Benchmarks

Build and run the complete Automerge-paper B4 trace:

```sh
pnpm --dir loro-js bench:b4 -- 259778 7
```

The script fully warms the largest requested prefix and releases the previous
sample before forced GC. It reports local editing plus snapshot/update import
and export. Run scaling probes for the main indexed structures and history APIs
with:

```sh
pnpm --dir loro-js bench:complexity -- 1000,2000,4000,8000
```

On an Apple M5 Pro with Node 26.4.0, the complete 259,778-action B4 trace now
applies in a 239.0 ms three-sample median (237.1–242.1 ms samples) and finishes
at 104,852 UTF-16 code units. The resulting process reported 107.1 MB of used JS
heap and 322.9 MB RSS.
The original array implementation was estimated at 30–50 minutes. Prefix
measurements from 20k through the full trace scale approximately linearly. The
matching Rust Criterion benchmark has a 47.711 ms point estimate on the same
machine, so TypeScript is about 5.0x slower in absolute time. B4 leaves 182,315
scalar objects but packs them into 13,613 TypeScript treap nodes. The same run
measured snapshot
export at 97.1 ms, update export at 86.5 ms, snapshot import at 80.0 ms, and
update import at 221.7 ms.

The `zxch3n/crdt-benchmarks` adapters provide a separate end-to-end comparison
against the published Loro WASM adapter. With the local `loro-js` build, B4 fell
from more than 180 seconds before the fixes (141.5 seconds after removing the
first copy path) to 2.846 seconds after incrementally maintaining merged-change
lengths; the WASM adapter result is 4.733 seconds. B3.5 takes 288 ms versus
303 ms. B3.3 emits a 240,032-byte snapshot versus roughly 242 KB from WASM,
down from the former 7.95 MB uncompressed TypeScript snapshot. A 60k-item List
update is 231,840 bytes and a 120k-character Text update is 120,095 bytes, both
matching the WASM output sizes. On C1.1, three consecutive local runs take a
5.688-second median (5.467–5.827 seconds), encode the snapshot in a 1.552-second
median, retain 207.4 MB, and parse it in a 1.357-second median. Before Text state
span coalescing and compact hydration, the same adapter took 6.988 seconds,
encoded in 1.961 seconds, retained 367.8 MB, and parsed in 3.620 seconds. The
snapshot shrank from about 6.51 MB to 6.26 MB. An isolated profile measures
state decode at 98 ms and core snapshot import at a 395 ms median; the
adapter's parse number also includes two forced garbage collections. The WASM
adapter remains faster at 1.728 seconds overall and 43 ms for parse, so the
remaining gap is a constant-factor and representation issue rather than a
public-API complexity regression.

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

- Multi-scalar Text operations use compact string/ID spans, but B4 inserts all
  182,315 scalars as separate operations. Its scalar fast path therefore still
  retains one object per inserted scalar. Bounded physical spans cut B4's
  treap-node count by about 13.4x; inline locations also remove one location
  object and WeakMap entry per scalar. Together with the run indexes they cut
  the early 913.5 ms scalar-object apply median by roughly two thirds. Matching
  Rust's memory use and constant factors needs a direct scalar-to-compact-span
  append path that does not construct a temporary span for every edit.
- A subscribed restoration of a large insertion or deletion must include the
  restored text/list values in its event, so its work is proportional to that
  emitted output. Without a subscriber, both hide and show transitions use the
  reversible lazy visibility layer and stay proportional to affected ID runs.
- Importing interleaved concurrent MovableList moves can canonicalize the
  affected container once. Initial snapshot hydration and fallback transitions
  with incomplete history can likewise materialize complete touched containers
  when their returned state or subscriber event requires it.
- The million-operation C1.1 concurrent-text trace still exposes a large
  constant-factor gap. Streaming DeltaRLE, typed LZ4 decode, deferred history,
  coalesced Text state spans, chunk hydration, and bounded dense counter storage
  removed the known superlinear and largest temporary-allocation failures. The
  remaining work is in edit-time insertion context and treap recomputation,
  validation-only frontier decoding, and a direct snapshot treap builder. These
  are internal representation improvements rather than public-API complexity
  changes.
- Old or manually constructed containers without a parent-edge binding scan
  their parent once, then cache the recovered binding. Normal container path
  lookup uses the indexed binding directly.

Keep randomized index-invariant coverage in `loro-js/tests/indexes.test.ts` and
Rust/TypeScript fixture coverage in `loro-js/tests/rust-interop.test.ts` when
changing these structures.
