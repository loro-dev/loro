# loro-crdt-map

## 1.15.1

### Patch Changes

- 34b1fae: Cache immutable container ids per JavaScript wrapper to avoid repeated WASM string decoding.
- 48b3c52: Keep malformed `cid:`-prefixed strings returned from `toJsonWithReplacer` as
  plain strings instead of throwing while resolving them as container IDs.

## 1.15.0

### Patch Changes

- 4837d07: Fix unbounded degradation when re-asserting identical text marks over styled ranges.

  `mark` already tried to skip no-op marks, but the check used
  `StyleRangeMap::get_styles_of_range`, which returns `None` whenever the marked
  range spans more than one style-range leaf. On any document whose style ranges
  are already fragmented (i.e. any document with styles), re-asserting an
  identical mark recorded a new op every time.

  Each redundant mark leaves a pair of style anchors in the container state
  forever — they survive snapshots and are never consolidated — and every styled
  read walks all of them. Callers that re-assert marks (for example editor
  bindings syncing mark state) therefore permanently degraded styled reads without
  bound: reading a 724-character paragraph went from ~5µs to ~4ms after 1000
  redundant marks.

  The skip path now falls back to `StyleRangeMap::range_has_key_value`, which
  scans every style range covering the mark and reports whether each position
  already resolves the key to the given value. Attached and detached texts both
  use the new path when the single-leaf fast path cannot answer.

- f4e011f: Fix an O(n^2) slowdown when reading styled text.

  Building the per-range style metadata for `toDelta` / `getRichtextValue` (and
  the sliced variants used to emit text events) deep-copied the whole `Styles`
  map once per style range. Each key in that map owns the set of _every_ style op
  covering the range, while the resulting `StyleMeta` keeps only the LWW winner,
  so the copy scaled with the number of marks accumulated on the container and
  then discarded all but one element of it — O(marks) per range across O(marks)
  ranges. The conversion now borrows instead of cloning.

  Style anchors are never consolidated, so marks accumulate in the container
  state and every styled read paid for all of them. On a 724-character paragraph
  carrying 2000 accumulated marks, a styled read drops from ~103ms to ~1ms; a
  simulated editor binding that re-asserts marks on each keystroke drops from
  ~5ms to ~200us per read after 500 keystrokes.

## 1.14.1

### Patch Changes

- 76cc0f9: Make `importBatch` atomic and never leave the document detached.

  `importBatch` force-detaches the document so each blob only touches the OpLog, then
  reattaches with a single checkout at the end. Two exits skipped that reattach and
  stranded the document in detached mode, where later imports stop being reflected in
  the state, local edits branch off a stale version, and every later `attach()` re-runs
  the same failing checkout:

  - The closing checkout was `expect`ed to succeed. Remote ops that decode fine but are
    rejected by state validation (for example a list insert past the end of the list)
    only reach that validation here, because the blobs were imported while detached, so
    a malformed blob turned into a panic — a `RuntimeError: unreachable` trap in WASM —
    after the document had already been left detached.
  - Any other panic while decoding or applying a blob unwound past the reattach.

  The batch now runs inside an OpLog rollback scope. If the closing checkout cannot
  apply the accumulated changes, the whole batch is rolled back and `importBatch`
  returns the state-apply error with the document still attached and unchanged, instead
  of trapping. A panic inside the batch reattaches before it is re-raised.

  The pending-changes rollback journal now snapshots each touched slot as it was when
  the scope began, instead of undoing individual mutations. The previous two-phase undo
  resurrected changes that the same import both parked and unlocked — routine across a
  batch — leaving pending entries that referenced container registrations the rollback
  had already discarded, and pre-batch pending changes unlocked by a rolled-back batch
  are now re-parked instead of silently dropped.

  `importBatch` of out-of-order updates is also considerably faster: the import preflight
  no longer rescans the whole pending set once per blob, which made such a batch
  quadratic. Draining 12000 out-of-order updates in one `importBatch` goes from ~3.4s to
  ~0.45s, with peak memory unchanged. In-order batches and single `import` calls are
  unaffected.

## 1.14.0

### Minor Changes

- bbf21c3: fix: avoid unreadable WASM traps (`RuntimeError: unreachable executed`) on invalid arguments. `getCursor` with an invalid `side`, `getEditorOf` at an out-of-range position, and accessing an unknown container (created by a newer version of loro-crdt) now throw catchable JS errors with readable messages or return `undefined` instead of trapping the WASM instance.

  feat: on a Rust panic, the full panic info (message, `file:line`, and the complete stack including the Rust call path) is now stored as a JS `Error` on `globalThis.__LORO_WASM_LAST_PANIC__` before the instance traps, so applications can recover readable diagnostics from a caught `RuntimeError`.

  fix: the WASM start hook was never invoked in the `bundler`/`browser`/`base64` builds (the glue checked the JS namespace instead of the wasm exports for `__wbindgen_start`), so no panic hook was installed there at all. The start hook now runs in every build target.

  feat: out-of-memory failures are now reported too. Allocation failure in WASM bypasses the panic hook and traps silently; the package now wraps the global allocator so OOM stores `loro-crdt: out of WASM memory: allocation of N bytes failed` on `globalThis.__LORO_WASM_LAST_PANIC__` and prints it to `console.error` before trapping.

### Patch Changes

- bbcaef6: Fix a panic on import that could permanently break a document's sync.

  `import_changes_to_oplog` defers the changes whose deps are not yet in the DAG
  and hands them to `extend_pending_changes_with_unknown_lamport` to be parked.
  `try_apply_pending` runs in between and applies changes parked by earlier
  imports, which advances the oplog version, so a deferred change can become
  applicable — or already applied — by the time it is parked. Both outcomes used
  to hit `unreachable!`, compiling to `RuntimeError: unreachable` in WASM.

  The re-classification is now handled instead of asserted: a change that became
  applicable is applied and cascaded into whatever it unblocks, an already-applied
  one is skipped, and `ImportStatus` reports only what genuinely stayed pending.

  This mattered most through `import_batch`, which force-detaches the document for
  the duration of the batch and reattaches after the loop. The panic unwound past
  the reattach and left the document detached, so every later import and export
  failed and the document stopped syncing until the process restarted.

## 1.13.9

### Patch Changes

- ddc47ec: Retreat the conservative replay base to the latest single-head critical
  version of the two versions' combined history instead of the beginning of
  history. When an import or checkout involves a genuinely concurrent branch,
  the causal replay now starts at the most recent point that no concurrency
  crosses (in the sense of Eg-walker's critical versions), skipping the
  fully-synced common prefix that the old empty-version fallback replayed.
- ddc47ec: Fix a convergence bug where a movable tree's incrementally maintained state
  could diverge from a full replay of its own oplog. When newly imported
  operations were concurrent with part of the receiving peer's multi-head
  frontier, the diff mode was misclassified as concurrency-free and the tree
  fast path applied the new moves without adjudicating them against the
  existing concurrent branch. The classifier now verifies that every entry
  point of the imported region causally covers the whole current frontier,
  and otherwise retreats the replay base to the latest critical version so
  the competing branches are replayed together.
- ddc47ec: Correct LCA unmatched-branch detection when an explicit dependency already
  covers the same peer's implicit predecessor. Keep those causally newer imports
  on the current replay base, and avoid rebuilding unchanged list-like containers
  when a genuinely concurrent update needs a conservative base containing a large
  common history.

## 1.13.8

### Patch Changes

- 033f43a: Keep the source-map package changelog aligned with the matching `loro-crdt` release.

## 1.13.7

### Patch Changes

- 3986805: Speed up snapshot import. When decoding a Loro snapshot, the redundant per-block SSTable validation (eager block-metadata decode and per-block checksums) is now skipped, because the whole snapshot body is already protected by the document-level checksum verified during decoding. This removes a second hash pass over the data (roughly halving B4 snapshot import time) while preserving integrity guarantees.

  This fast path is internal to Loro's snapshot decoding. The public `MemKvStore::import_all` still verifies every block's checksum; a separate `import_all_unchecked` opts into the unchecked path and is only used where an outer checksum already guarantees integrity.

- 9b6330d: Speed up local text editing (~35% faster on the B4 editing trace). Three hot-path
  changes: the lock-order debug instrumentation is now compiled out of release
  builds (it ran on every per-op lock acquisition); the visible-op count is bumped
  incrementally for local ops instead of recomputing it from the version vectors
  (which also allocated) on every op; and a couple of per-op allocations on the
  text insert/delete path were removed (lazy error-context formatting and inline
  storage for entity ranges).
- c37e76a: Fix a checkout hang after snapshot import. Change-store blocks decoded from a snapshot recorded a wrong end lamport (`lamport_range.1` was set to the start lamport of the block's last change instead of its end). When a change was split across multiple blocks and a lamport-based lookup engaged the binary search path in `ChangeStore::get_change_by_lamport_lte` — e.g. the movable-list diff calculator resolving historical positions during `checkout` — the degenerate range made the search loop forever. The binary search now also caps its steps and falls back to a scan if it ever fails to converge; the fallback considers both unflushed in-memory blocks and the underlying kv store, so lamport lookups (e.g. `getChangeAtLamport`) stay correct for local changes that have not been flushed yet.

## 1.13.6

### Patch Changes

- a4e6174: Fix undo/redo restoring mergeable map children as regular non-mergeable containers.
- 78f6c8a: Fix a potential deadlock when lazy snapshot loading resolves mergeable container parent depths.

## 1.13.5

### Patch Changes

- 1727258: Improve text insert and snapshot import performance by avoiding duplicate text boundary validation and skipping eager imported change block parsing.
- 52d8168: Recover two per-operation editing slowdowns regressed since 1.11.

  Both are constant-factor regressions on the per-op (auto-commit) editing path
  introduced by the lazy-snapshot work in #985, measured against the 1.11.1
  release.

  1. Every `MapHandler`/`ListHandler`/`MovableListHandler` insert validated its
     value with `ensure_no_regular_container_value`, which heap-allocated a `Vec`
     on each call even for scalar values (the common case). A scalar fast-path now
     skips the allocation and traversal entirely. `map create 10^4 key`:
     ~19.4ms -> ~10.7ms.
  2. The per-op text bounds check (`TextHandler::len`/`len_unicode`/`len_utf16`)
     took two `DocState` locks — one to check whether the container state was
     decoded, then another to query the length. These are now consolidated into a
     single `DocState::get_text_len` that takes one lock and one container-store
     lookup. The lazy-snapshot memory behavior is preserved: a still-lazy
     container reads its cached length metadata without materializing the full
     richtext state. `bench_text B4 apply` (per-op text editing): ~389ms -> ~352ms.

## 1.13.4

### Patch Changes

- 4d577ad: Fix two O(n^2) editing slowdowns.

  1. Editing with UTF-16 / UTF-8 (byte) positions (the default in the JS binding)
     validated each position by materializing the entire `[0, pos)` prefix string,
     making every `insert`/`delete`/`splice`/`mark` O(n) and a run of edits O(n^2)
     (regression since 1.12.0). The boundary check now reads the rope's prefix
     caches via the cursor (O(log n)). Unicode-indexed editing was unaffected.
  2. When a subscriber is attached and many edits land on the same container within
     one event batch (e.g. random-position inserts, or many distinct map-key
     writes), building the event cloned the growing accumulated diff on every
     compose — O(n^2) in the number of fragments. The diffs are now composed in
     place. This affected text, map and list events.
  3. Converting a UTF-16 / UTF-8 position within a text chunk to a unicode offset
     scanned the chunk char-by-char, so editing/slicing a large contiguous chunk
     (a big insert, a loaded document, or a long run of typed text that merges into
     one chunk) was O(chunk length) per op. Chunks that contain no astral-plane
     characters (UTF-16) or are pure ASCII (UTF-8) now convert in O(1), covering
     essentially all real-world text (ASCII/Latin/CJK).

## 1.13.0

### Minor Changes

- fa888d8: Add mergeable child containers: child containers created under a map key that converge across peers on concurrent first-write instead of forking. Exposed as `ensureMergeable{Counter,Map,List,MovableList,Text,Tree}` on `LoroMap`. A mergeable child lives at a deterministic `ContainerID` derived from `(parent, key, kind)`, and its visibility is driven by a binary ref the parent map stores at the key, so deletes and kind conflicts resolve through the map's regular LWW. `getOrCreateContainer` is deprecated for lazy map-child creation; migrate those call sites to `ensureMergeable*`. `setContainer` and `insertContainer` regular semantics are unchanged.

## 1.12.4

### Patch Changes

- a6e23b6: Optimize snapshot export for shallow documents by reusing cached shallow-root state instead of checking out to the shallow root and back to latest.

## 1.12.2

### Patch Changes

- cc587ed: Add a browser package remapping so Vite/Rolldown production builds load WASM without top-level await or circular wasm wrapper chunks.

  Also make the base64 entry easier to bundle with plain esbuild, Rollup, and Next.js Webpack by avoiding static Node builtin `require()` calls and top-level await in browser bundles.

- 8f57f4c: Reduce memory usage for read-only access to snapshot-imported documents by avoiding unnecessary lazy container state initialization.

## 1.12.0

### Minor Changes

- 7dfda87: Make update imports atomic across oplog and document state application.

  - `import` and `import_json_updates` now roll back imported oplog changes when state application fails, so malformed updates do not leave the document with oplog/state divergence.
  - Pending changes that are activated during import are included in the rollback boundary when they can affect state application.
  - Import rollback uses conditional guards to avoid adding fixed overhead to successful detached or no-op imports.

### Patch Changes

- 64aa97c: Harden encoding, snapshot, and import paths against malformed input

  - JSON schema import (`import_json_updates`): out-of-range compressed peer indices now return `DecodeError` instead of being silently accepted as raw peer IDs; mismatched `JsonOpContent` vs container type returns `DecodeError` instead of panicking.
  - Outdated binary encoding decoder (`decode_op`): malformed op streams (missing delete iterators, type mismatches) now return `DecodeDataCorruptionError` instead of panicking.
  - Fast snapshot decoder (`decode_snapshot_blob_meta`): truncated or oversized section lengths now return `DecodeDataCorruptionError` instead of panicking on slice indexing.
  - Change store KV import (`import_all`): corrupted `VersionVector`/`Frontiers` metadata now returns `DecodeDataCorruptionError` instead of panicking.
  - Value encoding (`LoroValueKind::from_u8`, `read_str`): invalid byte values and invalid UTF-8 now return `DecodeDataCorruptionError` instead of panicking.
  - `LoroDoc::diff()`: checkout failures during diff calculation are now propagated as `LoroError` instead of panicking; state restore uses `unwrap()` to fail-fast on internal errors.
  - `try_get_text/list/map/tree/movable_list/counter`: now return `None` for wrong root container types instead of panicking.
  - Detached list insert out-of-bounds: returns `LoroError::OutOfBound` instead of panicking.
  - Tree `mov_after`/`mov_before` on deleted node: returns `TreeNodeDeletedOrNotExist` instead of panicking.
  - `JsonChange::op_len`: empty ops array returns `0` instead of panicking.
  - `renew_peer_id`: avoids theoretical collision with `PeerID::MAX`.

- 0977ad1: Fix lock-order panics when JavaScript callbacks re-enter Loro APIs.

  - `opCount()` no longer reacquires the OpLog lock while the current thread already holds a higher-order lock.
  - `LoroText.iter()` snapshots text chunks before invoking the user callback, so callback code can safely read or mutate the document.

- ef100e6: Reduce memory spikes when exporting snapshots from shallow documents.

  When a shallow document is re-exported from its existing shallow root with only a small tail of updates, Loro now reuses the stored shallow-root state instead of decoding all containers just to re-encode the same state.

- 933d5d6: feat: add clearRedo and clearUndo methods
  #921
- 17dc6c0: Fix several edge-case contract violations in document, text, and JSONPath APIs.

  - JSONPath `value(...)` comparisons now handle boolean values consistently with other scalar comparisons.
  - Rich text mark expansion now follows `ExpandType::Before` and `ExpandType::Both` at documented insertion boundaries.
  - Text delta slicing now validates invalid ranges and UTF-8/UTF-16 boundaries before slicing, and public deltas omit removed-style tombstones after unmarking.
  - Detached list and movable-list out-of-bounds operations now return `LoroError::OutOfBound` instead of panicking.

## 1.11.1

### Patch Changes

- 6f5b7a9: Fix production panic regressions

## 1.10.8

### Patch Changes

- 9f68a57: Return errors instead of panicking when diff or checkout targets frontiers before a shallow snapshot root.

## 1.10.6

### Patch Changes

- 85921d7: fix: don't hang when remapping nested containers w same ID (#911)

## 1.10.5

### Patch Changes

- 2d6c235: Fix counter undo after remote updates #906

## 1.10.4

### Patch Changes

- 864b5ca: perf(loro-internal): remove quadratic slow paths in text import/checkout #895
- a34134d: perf: rm the event calling wrapper for ops that will not trigger events

## 1.10.3

### Patch Changes

- 2800a4c: perf: skip useless unmark op #878
- fffaf45: feat: add JSONPath subscription #883

## 1.10.2

### Patch Changes

- 53635dd: fix: toDelta should ignore null style entries #875

## 1.10.1

### Patch Changes

- ca76d86: fix: Empty LoroText attach error #873

## 1.10.0

### Minor Changes

- ce16b52: feat: add sliceDelta method to slice a span of richtext #862

  Use `text.sliceDelta(start, end)` when you need a Quill-style delta for only part of a rich text field (for example, to copy a styled snippet). The method takes UTF-16 indices; use `sliceDeltaUtf8` if you want to slice by UTF-8 byte offsets instead.

  ```ts
  import { LoroDoc } from "loro-crdt";

  const doc = new LoroDoc();
  doc.configTextStyle({
    bold: { expand: "after" },
    comment: { expand: "none" },
  });
  const text = doc.getText("text");

  text.insert(0, "Hello World!");
  text.mark({ start: 0, end: 5 }, "bold", true);
  text.mark({ start: 6, end: 11 }, "comment", "greeting");

  const snippet = text.sliceDelta(1, 8);
  console.log(snippet);
  // [
  //   { insert: "ello", attributes: { bold: true } },
  //   { insert: " " },
  //   { insert: "Wo", attributes: { comment: "greeting" } },
  // ]
  ```

### Patch Changes

- a78d70f: fix: avoid convert panic #858
- ee94ee4: fix: EphemeralStore apply should ignore timeout entries #865
- 9e0a613: fix: Reject symbol-keyed map objects in wasm conversion #855

## 1.9.0

### Minor Changes

- 10a405b: feat: JSONPath rfc9535 #848

  Thanks to @zolero for the thorough implementation of JSONPath support!

  LoroDoc now supports querying and mutating document data using **JSONPath**, following the [RFC 9535](https://www.rfc-editor.org/rfc/rfc9535) specification.

  ### 🧩 API

  ```ts
  // Execute a JSONPath query on the document
  doc.JSONPath(path: string): any[];
  ```

  ### 📚 Query Examples

  Example data setup

  ```ts
  const doc = new LoroDoc();
  const store = doc.getMap("store");

  // Simplified setup for illustration purposes
  store.set("books", [
    {
      title: "1984",
      author: "George Orwell",
      price: 10,
      available: true,
      isbn: "978-0451524935",
    },
    {
      title: "Animal Farm",
      author: "George Orwell",
      price: 8,
      available: true,
    },
    {
      title: "Brave New World",
      author: "Aldous Huxley",
      price: 12,
      available: false,
    },
    {
      title: "Fahrenheit 451",
      author: "Ray Bradbury",
      price: 9,
      available: true,
    },
    {
      title: "The Great Gatsby",
      author: "F. Scott Fitzgerald",
      price: null,
      available: true,
    },
    {
      title: "To Kill a Mockingbird",
      author: "Harper Lee",
      price: 11,
      available: true,
    },
    {
      title: "The Catcher in the Rye",
      author: "J.D. Salinger",
      price: 10,
      available: false,
    },
    {
      title: "Lord of the Flies",
      author: "William Golding",
      price: 9,
      available: true,
    },
    {
      title: "Pride and Prejudice",
      author: "Jane Austen",
      price: 7,
      available: true,
    },
    {
      title: "The Hobbit",
      author: "J.R.R. Tolkien",
      price: 14,
      available: true,
    },
  ]);
  store.set("featured_authors", ["George Orwell", "Jane Austen"]);
  ```

  ```ts
  // 1. Get all book titles
  doc.JSONPath("$.store.books[*].title");
  // → ["1984", "Animal Farm", "Brave New World", "The Hobbit"]

  // 2. Filter: available books only
  doc.JSONPath("$.store.books[?(@.available)].title");
  // → ["1984", "Animal Farm", "The Hobbit"]

  // 3. Filter: books with price > 10
  doc.JSONPath("$.store.books[?(@.price > 10)].title");
  // → ["The Hobbit"]

  // 4. Use recursive descent to get all prices
  doc.JSONPath("$..price");
  // → [10, 8, 12, 9, null, 11, 14]

  // 5. Slice syntax: first three books
  doc.JSONPath("$.store.books[0:3].title");
  // → ["1984", "Animal Farm", "Brave New World"]

  // 6. Membership test: authors in featured list
  doc.JSONPath("$.store.books[?(@.author in $.store.featured_authors)].title");
  // → ["1984", "Animal Farm", "Pride and Prejudice"]

  // 7. String match using `contains`
  doc.JSONPath("$.store.books[?(@.title contains 'The')].author");
  // → ["F. Scott Fitzgerald", "J.R.R. Tolkien"]
  ```

- 10a405b: refactor!: remove deprecated encoding format in v0.x #849

### Patch Changes

- 3af6a85: fix: WASM loading compatibility for esbuild and rsbuild #851

## 1.8.9

### Patch Changes

- 53f5533: Extract sourcemap to another package
