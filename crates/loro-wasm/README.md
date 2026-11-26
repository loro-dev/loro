<p align="center">
  <a href="https://loro.dev">
    <picture>
      <img src="./docs/Loro.svg" width="200"/>
    </picture>
  </a>
</p>
<h1 align="center">
<a href="https://loro.dev" alt="loro-site">Loro</a>
</h1>
<p align="center">
  <b>Make your JSON data collaborative and version-controlled 🦜</b>
</p>
<p align="center">
  <a href="https://trendshift.io/repositories/4964" target="_blank"><img src="https://trendshift.io/api/badge/repositories/4964" alt="loro-dev%2Floro | Trendshift" style="width: 250px; height: 55px;" width="250" height="55"/></a>
</p>
<p align="center">
  <a href="https://loro.dev/docs">
    <b>Documentation</b>
  </a>
  |
  <a href="https://loro.dev/docs/tutorial/get_started">
    <b>Getting Started</b>
  </a>
  |
  <a href="https://docs.rs/loro">
    <b>Rust Doc</b>
  </a>
</p>
<p align="center">
  <a aria-label="X" href="https://x.com/loro_dev" target="_blank">
    <img alt="" src="https://img.shields.io/badge/Twitter-black?style=for-the-badge&logo=Twitter">
  </a>
  <a aria-label="Discord-Link" href="https://discord.gg/tUsBSVfqzf" target="_blank">
    <img alt="" src="https://img.shields.io/badge/Discord-black?style=for-the-badge&logo=discord">
  </a>
</p>


<h4 align="center">
  ✨ Loro 1.0 is out! Read the <a href="https://loro.dev/blog/v1.0">announcement</a>.
</h4>

Loro is a [CRDTs(Conflict-free Replicated Data Types)](https://crdt.tech/) library that makes building [local-first][local-first] and collaborative apps easier. You can now use it in Rust, JS (via WASM), and Swift.

# Features

**Features Provided by CRDTs**

- P2P Synchronization
- Automatic Merging
- Local Availability
- Scalability
- Delta Updates

**Supported CRDT Algorithms**

- 📝 Text Editing with [Fugue]
- 📙 [Rich Text CRDT](https://loro.dev/blog/loro-richtext)
- 🌲 [Moveable Tree](https://loro.dev/docs/tutorial/tree)
- 🚗 [Moveable List](https://loro.dev/docs/tutorial/list)
- 🗺️ [Last-Write-Wins Map](https://loro.dev/docs/tutorial/map)
- 🤝 Mergeable map-key children via `ensureMergeable*` for lazy child container creation

**Advanced Features in Loro**

- 🚀 [Fast Document Loading](https://loro.dev/blog/v1.0)
- ⏱️ Fast [Time Travel](https://loro.dev/docs/tutorial/time_travel) Through History
- 🏛️ [Version Control with Real-Time Collaboration](https://loro.dev/blog/v1.0#version-control)
- 📦 [Shallow Snapshot](https://loro.dev/docs/advanced/shallow_snapshot) that Works like Git Shallow Clone 


> In this example, we demonstrate importing an entire Loro codebase into a Loro-powered 
> version controller, preserving the complete Git DAG history while enabling fast version switching.

## Undo and map child identity

`UndoManager` selects changes made by the bound local peer. When undoing the
deletion of a map key that held a container, the child's creation method matters:

- A regular child created with `map.setContainer(key, new LoroMap())` is
  reconstructed with a **new container ID**. Existing handles still address the
  original child. Concurrent edits sent to the original ID remain there and do
  not appear in the reconstructed child, whether imported before or after undo.
- A mergeable child created with `map.ensureMergeableMap(key)` (or another
  `ensureMergeable*` method) keeps the same parent/key/type identity when undo
  restores the key. Retained state and concurrent edits become visible again.
  The parent must keep its ID: if undo reconstructs a regular ancestor, its
  mergeable descendants also get different IDs.

For example, A deletes a regular child containing `{ base: 1 }` while B adds
`remote: 2` to that child's original ID. A's undo restores a new child containing
`{ base: 1 }`; B's edit still targets the hidden original child. Both replicas
converge to this result. With a mergeable child under the same parent, undo
restores the same ID and the visible result is `{ base: 1, remote: 2 }` in either
import order. Undo's ID remapping does not redirect imported operations.

Use mergeable children for a stable logical map slot. Reusing the same
parent/key/type resurfaces retained state rather than creating an empty entity.
Use distinct keys for distinct entity lifetimes. To make entity deletion
undoable while keeping its identity and concurrent edits, retain the entity's
container and undo a visibility or `deleted` field instead of deleting its map key.

## Debugging the Wasm build

The standard build pipeline (`deno run -A ./scripts/build.ts dev|release`) now keeps DWARF debugging information through `wasm-bindgen` and emits two helper files alongside every `loro_wasm_bg.wasm` artifact:

- `loro_wasm_bg.wasm.map` &mdash; a v3 source map derived from DWARF so that Chrome, Edge, and Firefox can show original Rust locations when inspecting stack traces.

Load the source map in browser devtools; when devtools fetches the debug companion it can map instructions back to Rust source files and line numbers without inflating the shipped `.wasm`.

## Bundler entries

Bare `import { LoroDoc } from "loro-crdt"` uses package conditional exports. Browser development builds can resolve the nested `browser` + `development` conditions to the bundler entry, while browser production builds can resolve the `browser` condition to a synchronous browser build that avoids Vite/Rolldown production chunk cycles around `.wasm` wrappers. A legacy `browser` string field also points to the browser entry for bundlers that still consult it. Runtimes that need native `.wasm` module imports can still use the `bundler` entry, and apps that prefer explicit async initialization can use `loro-crdt/web`.

Vite and Webpack understand `new URL("./loro_wasm_bg.wasm", import.meta.url)` and emit the WASM asset automatically. Plain esbuild and plain Rollup do not copy that asset by default. For those tools, either import `loro-crdt/base64` to inline the WASM into the JS bundle without top-level await, or keep the default `loro-crdt` import and copy `node_modules/loro-crdt/browser/loro_wasm_bg.wasm` next to the emitted JS bundle as a build step.

Next.js Turbopack can use the default browser entry. If a Next.js Webpack build resolves the `bundler` entry instead of the package `browser` remap, use `loro-crdt/base64`.

# Example

[![Open in StackBlitz](https://developer.stackblitz.com/img/open_in_stackblitz.svg)](https://stackblitz.com/edit/loro-basic-test?file=test%2Floro-sync.test.ts)

```ts
import { expect, test } from 'vitest';
import { LoroDoc, LoroList } from 'loro-crdt';

test('sync example', () => {
  // Sync two docs with two rounds of exchanges

  // Initialize document A
  const docA = new LoroDoc();
  const listA: LoroList = docA.getList('list');
  listA.insert(0, 'A');
  listA.insert(1, 'B');
  listA.insert(2, 'C');

  // Export all updates from docA
  const bytes: Uint8Array = docA.export({ mode: 'update' });

  // Simulate sending `bytes` across the network to another peer, B

  const docB = new LoroDoc();
  // Peer B imports the updates from A
  docB.import(bytes);

  // B's state matches A's state
  expect(docB.toJSON()).toStrictEqual({
    list: ['A', 'B', 'C'],
  });

  // Get the current version of docB
  const version = docB.oplogVersion();

  // Simulate editing at B: delete item 'B'
  const listB: LoroList = docB.getList('list');
  listB.delete(1, 1);

  // Export the updates from B since the last sync point
  const bytesB: Uint8Array = docB.export({ mode: 'update', from: version });

  // Simulate sending `bytesB` back across the network to A

  // A imports the updates from B
  docA.import(bytesB);

  // A has the same state as B
  expect(docA.toJSON()).toStrictEqual({
    list: ['A', 'C'],
  });
});
```

# Things to know

These come up often when people start using `loro-crdt`.

## Events fire on commit

`doc.subscribe` and container `subscribe` callbacks run when a transaction is
committed: on `doc.commit()`, or implicitly on `export`, `import` and
`checkout`. An edit you haven't committed sends no event, and
`subscribeLocalUpdates` doesn't include it yet.

## Text positions are UTF-16 indexes

`LoroText.insert`, `delete`, `slice`, `charAt` and text events count UTF-16
code units, the same as JS string indexes. Convert positions that come from
UTF-8 or code point offsets first.

## One peer ID per session

Each `LoroDoc` starts with a random peer ID. If you call `setPeerId`, don't give
the same ID to two sessions that can edit at the same time, and don't use a user
ID: one user can have several tabs or devices open. Peers that share an ID send
updates that look like duplicates, and the docs stop converging.

## Concurrent `setContainer` on one map key keeps one child

`setContainer` and `getOrCreateContainer` make a regular child whose ID comes
from the operation that created it. When two peers create a child at the same
key at the same time, one wins and the other child's content is hidden. If more
than one peer can create the child, use `ensureMergeableMap`,
`ensureMergeableText` or another `ensureMergeable*` method, and every peer gets
the same child.

## `checkout` makes the document read-only

After `doc.checkout(frontiers)` the document is detached. It shows the old
version, edits throw, and imports go into history without changing what you
see. `doc.checkoutToLatest()` (or `attach()`) brings it back. To restore an old
version as new edits, use `doc.revertTo(frontiers)`.

## Container handles keep the document alive after `doc.free()`

Every container object you get from a document (`doc.getText()`, `map.get()`,
`list.insertContainer()` and so on) holds a reference to the document. A
detached container (`new LoroText()`, `new LoroMap()`, ...) does too once you
attach it with `setContainer` or `insertContainer`, because it keeps its
attached copy (see `getAttached()`). `doc.free()` only frees the `LoroDoc`
object. The document's memory goes once every handle is gone too, either from
its own `free()` or from the garbage collector finalizing it.

Finalizers run only after the current task yields to the event loop. Code that
creates and drops many documents in one synchronous loop (a test suite, a batch
job, a request handler that never awaits) keeps all of them in WASM memory
until it yields, and WASM memory never shrinks. In loro-dev/loro#1312, 1,000
documents with a 20,000-character text each grew WASM memory by about 44 MB
when only `doc.free()` was called. Freeing the text handle too kept it flat.
Free the handles first, or let the loop yield:

```ts
const doc = new LoroDoc();
const text = doc.getText("text");
text.insert(0, "hello");
doc.commit();

text.free();
doc.free();
```

A handle still works after `doc.free()` while it's alive, since it keeps the
document.

## Undoing a deleted map key makes a new child container

A child made with `setContainer` comes back under a new container ID when undo
restores its key. One made with `ensureMergeable*` keeps its ID. See
[Undo and map child identity](#undo-and-map-child-identity).

# Blog

- [Loro 1.0](https://loro.dev/blog/v1.0)
- [Movable tree CRDTs and Loro's implementation](https://loro.dev/blog/movable-tree)
- [Introduction to Loro's Rich Text CRDT](https://loro.dev/blog/loro-richtext)
- [Loro: Reimagine State Management with CRDTs](https://loro.dev/blog/loro-now-open-source)

# Credits

Loro draws inspiration from the innovative work of the following projects and individuals:

- [Diamond-types](https://github.com/josephg/diamond-types): The [Event Graph Walker (Eg-walker)](https://loro.dev/docs/advanced/event_graph_walker) algorithm from @josephg has been adapted to reduce the computation and space usage of CRDTs.
- [Automerge](https://github.com/automerge/automerge): Their use of columnar encoding for CRDTs has informed our strategies for efficient data encoding.
- [Yjs](https://github.com/yjs/yjs): We have incorporated a similar algorithm for effectively merging collaborative editing operations, thanks to their pioneering work.
- [Matthew Weidner](https://mattweidner.com/): His work on the [Fugue](https://arxiv.org/abs/2305.00583) algorithm has been invaluable, enhancing our text editing capabilities.
- [Martin Kleppmann](https://martin.kleppmann.com/): His work on CRDTs has significantly influenced our comprehension of the field.
 

[local-first]: https://www.inkandswitch.com/local-first/
[Fugue]: https://arxiv.org/abs/2305.00583
