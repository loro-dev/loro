# loro-js Performance Architecture

Verified against code 2026-07-22.

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
  because moves break the origin-tree physical preorder.
- Merging adjacent changes appends only the new operations and key-table entries
  to the retained record. The cached operation length, peer end, frontier set,
  operation indexes, and subscriber update slice are updated incrementally, so
  a stream of mergeable commits does not repeatedly copy or reduce its complete
  history. Consecutive List/MovableList inserts in one transaction also share a
  single operation value array.
- Local-update encoding uses number arithmetic for safe-range LEB128/Postcard
  integers, exact-size buffers for the final document and change block, and
  canonical direct encoders for the common one-change/one-operation shape.
  Empty tables share an immutable zero-length byte array. General multi-change
  and wide-integer paths keep using the regular codec implementation.
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
applies in a 257.0 ms seven-sample median and finishes at 104,852 UTF-16 code
units. The resulting process reported 107.4 MB of used JS heap and 323.4 MB RSS.
The original array implementation was estimated at 30–50 minutes. Prefix
measurements from 20k through the full trace scale approximately linearly. The
matching Rust Criterion benchmark has a 47.711 ms point estimate on the same
machine, so TypeScript is about 5.4x slower in absolute time. B4 leaves 182,315
scalar objects but packs them into 13,613 TypeScript treap nodes. The same run
measured snapshot export at 87.0 ms, update export at 58.7 ms, snapshot import
at 120.6 ms, and update import at 278.1 ms.

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

A fresh-process B4 probe isolates the cost of committing and publishing every
individual delete/insert. Seven paired runs with alternating order measured
1,873.1 ms at `bdfe9874` and 1,012.4 ms with the small-update codec paths, a
46.0% median reduction with all seven pairs faster. Without a local-update
subscriber, the medians were 621.6 ms and 580.3 ms (6.6%); applying the whole
trace and committing only once measured 264.5 ms and 262.2 ms, which is within
noise. All 259,778 published updates were byte-for-byte identical between the
two builds. After forced GC, the retained document heap differed by less than
1 KiB; external and ArrayBuffer memory were identical. The main ESM remains
485.75 kB / 92.11 kB gzip, while the lazy codec/state chunk grows by 9.43 kB raw
and 1.53 kB gzip. The final CPU profile is dispersed: GC is 8.4% and no remaining
JavaScript self-time bucket exceeds 5.5%, so further local micro-optimizations
need a new workload or a structural change to justify their complexity.

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
changing these structures.
