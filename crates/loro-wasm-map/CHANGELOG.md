# loro-crdt-map

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
