# loro.js

`loro.js` is an experimental, pure TypeScript implementation of the current Loro
binary format and CRDT runtime. It targets wire and API compatibility with
[`loro-crdt`](https://www.npmjs.com/package/loro-crdt) without loading WebAssembly.

The package has no asynchronous initialization step and exposes two entry points:

- `loro.js`: the `LoroDoc` runtime and container APIs.
- `loro.js/codec`: low-level binary format readers and writers.

The published ESM output targets ES2022. Consumers need a runtime or bundler
that supports ES2022 syntax and built-ins.

## Install

```sh
pnpm add loro.js
```

## Synchronize documents

```ts
import { LoroDoc } from "loro.js";

const alice = new LoroDoc();
alice.setPeerId(1);
alice.getText("body").insert(0, "hello");
alice.getMap("meta").set("published", false);

const bob = new LoroDoc();
bob.setPeerId(2);
bob.import(alice.export({ mode: "update" }));

bob.getText("body").push(" world");
alice.import(bob.export({ mode: "update", from: alice.oplogVersion() }));

console.log(alice.toJSON());
// { body: "hello world", meta: { published: false } }
```

`LoroMap`, `LoroList`, `LoroMovableList`, `LoroText`, `LoroTree`, and `LoroCounter`
are available as attached or detached containers. The runtime also includes snapshots,
shallow snapshots, checkout and fork, cursors, rich-text deltas, mergeable children,
JSON updates, JSONPath, diffs, events, undo/redo, `Awareness`, and `EphemeralStore`.

## Navigate text by line

`LoroText` builds its line index on the first line query and maintains it through later
edits, imports, and checkouts. Positions use UTF-16 code units, matching the rest of the
text API. A line break is `\n`; `getLine()` removes the preceding `\r` from CRLF text.

```ts
const text = alice.getText("body");

console.log(text.lineCount);
console.log(text.lineStart(10));
console.log(text.lineAt(120));
console.log(text.getLine(10));
```

After an unusually fragmented edit workload, `text.compact()` rebuilds adjacent
physical text spans without changing visible content or CRDT history.

## Read and rewrite the binary format

```ts
import {
  decodeChangeBlock,
  decodeFastUpdates,
  encodeChangeBlock,
  encodeFastUpdates,
} from "loro.js/codec";

const blocks = decodeFastUpdates(bytes);
const rewritten = encodeFastUpdates(
  blocks.map((block) => encodeChangeBlock(decodeChangeBlock(block))),
);
```

The codec entry point also exposes the document envelope, change blocks, state
snapshots, SSTables, serde-columnar data, postcard values, LZ4 blocks, XXHash32,
container IDs, frontiers, version vectors, and primitive byte readers/writers.

## Compatibility status

The following paths are checked against the Rust implementation:

- Current FastUpdates (mode 4) import, export, decode, and re-encode.
- Current FastSnapshot (mode 3), including full and shallow snapshots.
- Map, list, movable-list, text/rich-text, tree, counter, nested-container, and
  mergeable-container state.
- Concurrent Fugue text updates, concurrent container updates, and cursor bytes.
- JSON update import/export and the postcard Awareness/EphemeralStore protocols.
- Rust-produced fixtures imported by TypeScript and TypeScript-produced fixtures
  imported by Rust.
- Randomized multi-peer text and rich-text edits run side by side with the
  `loro-crdt` WASM build (`tests/richtext-differential.test.ts`): positions, deletes,
  marks with every expand mode, concurrency, checkout, snapshots, cursors, events,
  and `revertTo`. Style anchors are text-sequence elements, as in Rust, so every op
  position counts them.

This is not yet a claim of complete behavioral equivalence with every `loro-crdt`
edge case. Important current limits are:

- Legacy/outdated Loro update and snapshot modes are not decoded; use a current Loro
  release to migrate them first.
- Encoded updates and snapshots are wire-compatible, but their byte layout is not
  canonical. TypeScript output can differ byte-for-byte and be larger than Rust output.
- Importing a snapshot into a document that already has history is less complete than
  importing into a new document or using `LoroDoc.fromSnapshot()`.
- `diff()` preserves parent-before-child ordering, but independent containers at the
  same depth can appear in a different order than Rust's internal hash-map iteration.
- `UndoManager` uses ID-span-based semantic undo for sequence edits, marks, and
  common map/tree/counter changes. It does not transform an undo against later remote
  edits like Rust's, and movable-list move/set undo still uses simplified behavior.
- The hardest concurrent movable-list cases still use simplified metadata compared
  with the Rust implementation.
- `subscribeJsonpath()` deliberately uses broad invalidation, so its callback can have
  false positives.

Treat the package as experimental when data can be produced by untrusted or older
clients. Keep a `loro-crdt` interoperability test for the formats and operations your
application depends on.

## Upgrading from 0.2

1.0 reads every document the way Rust (`loro-crdt`) does, including documents
written by 0.2. The data has no version marker, so a document edited only with 0.2
can read differently after the upgrade:

- **Concurrent text inserts.** 0.2 could order an insert after a sibling's subtree
  differently from Rust. 1.0 orders it like Rust, so such a history can give other
  text (`bé9a` in 0.2, `béa9` in 1.0 and Rust).
- **Rich text.** 0.2 counted text positions without a mark's start and end anchors;
  Rust and 1.0 count them. Every insert, delete, and mark op that 0.2 wrote after a
  mark can therefore apply elsewhere (with bold on `ab` in `abcd`, inserting `X` at
  3 and deleting 1 gave `acXd` in 0.2; its ops read as `bXcd` in 1.0 and Rust), and
  styles can cover other text.
- **Cursors.** 0.2 encoded the UTF-16 position of a cursor's target; Rust and 1.0 use
  the Unicode position. A cursor encoded by 0.2 can resolve to another offset (on
  `ab😀`, 4 in 0.2 and 2 in 1.0). Get new cursors from the upgraded document.

Documents also edited by `loro-crdt` peers already disagreed with Rust in these cases;
1.0 makes them agree.

How 1.0 reads data written by 0.2 (measured on the examples above):

| 0.2 data              | 1.0 reads                                                                       |
| --------------------- | ------------------------------------------------------------------------------- |
| Updates, JSON updates | Rust's reading: `béa9`, `bXcd`                                                  |
| Full/shallow snapshot | The 0.2 text (`bé9a`, `acXd`) as current state, but the history reads like Rust |

A document loaded from a 0.2 snapshot therefore holds a current state that its own
history disagrees with. Its current state is safe to read right after the import,
but do not keep using the document: exporting it as updates gives `béa9`, a
replica loaded from updates holds other text and stays different after new edits
(`béQ9aZ` and `béQa9Z`), and checking out older versions and back can change the
content (Rust can then even show text that matches neither reading).

**Migrate every replica the same way.** Recommended paths:

1. **Keep the content, drop the history (safest).** Read the final state, either
   with 0.2 after checking out the latest version, or with 1.0 right after importing
   a 0.2 snapshot: `doc.toJSON()`, or `text.toDelta()` for rich text. Build one new
   document from it in 1.0 and share that document with every replica. Do not
   import old updates or snapshots into it.
2. **Keep the history, accept Rust's reading.** Import the 0.2 updates (not
   snapshots) on every replica, and check the content, because it can differ from
   what 0.2 showed.

## Development checks

From this directory:

```sh
pnpm test
pnpm exec tsgo -p tsconfig.json --noEmit
pnpm lint
pnpm build
pnpm fixtures:rewrite
```

From the repository root, the Rust-side compatibility suite is:

```sh
cargo test -p loro --test loro_js_interop
```

The format implementation follows the repository reference in
[`docs/encoding.md`](https://github.com/loro-dev/loro/blob/main/docs/encoding.md).
