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
- `UndoManager` uses ID-span-based semantic undo for sequence edits and common
  map/tree/counter changes. Style-only rich-text edits and movable-list move/set undo
  still use simplified behavior.
- The hardest overlapping rich-text-anchor and concurrent movable-list cases still use
  simplified metadata compared with the Rust implementation.
- `subscribeJsonpath()` deliberately uses broad invalidation, so its callback can have
  false positives.

Treat the package as experimental when data can be produced by untrusted or older
clients. Keep a `loro-crdt` interoperability test for the formats and operations your
application depends on.

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
