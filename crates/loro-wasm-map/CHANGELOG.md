# loro-crdt-map

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
