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
  <b>Reimagine state management with CRDTs 🦜</b><br/>
  Make your app state synchronized and collaborative effortlessly.
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


https://github.com/loro-dev/loro/assets/18425020/fe246c47-a120-44b3-91d4-1e7232a5b4ac

Loro is a [CRDTs(Conflict-free Replicated Data Types)](https://crdt.tech/) library that makes building [local-first apps][local-first] easier. It is currently available for JavaScript (via WASM) and Rust developers. 

Explore our vision in our blog: [**✨ Reimagine State Management with CRDTs**](https://loro.dev/blog/loro-now-open-source).

# Features

**Basic Features Provided by CRDTs**

- P2P Synchronization
- Automatic Merging
- Local Availability
- Scalability
- Delta Updates

**Supported CRDT Algorithms**

- 📝 Text Editing with [Fugue]
- 📙 [Peritext-like Rich Text CRDT](https://loro.dev/blog/loro-richtext)
- 🌲 [Moveable Tree](https://loro.dev/docs/tutorial/tree)
- 🚗 [Moveable List](https://loro.dev/docs/tutorial/list)
- 🗺️ [Last-Write-Wins Map](https://loro.dev/docs/tutorial/map)
- 🔄 [Replayable Event Graph](https://loro.dev/docs/advanced/replayable_event_graph)

**Advanced Features in Loro**

- ⏱️ Fast [Time Travel](https://loro.dev/docs/tutorial/time_travel) Through History
- 🏛️ [Version Control with Real-Time Collaboration](https://loro.dev/blog/v1.0#version-control)
- 📦 [Shallow Snapshot](https://loro.dev/docs/advanced/shallow_snapshot) that Works like Git Shallow Clone 

https://github.com/user-attachments/assets/68e0017a-4987-4f71-b2cf-4ed28a210987

# Example

[![Open in StackBlitz](https://developer.stackblitz.com/img/open_in_stackblitz.svg)](https://stackblitz.com/edit/loro-basic-test?file=test%2Floro-sync.test.ts)

```ts
import { expect, test } from 'vitest';
import { LoroDoc, LoroList } from 'loro-crdt';

test('sync example', () => {
  /**
   * Demonstrates synchronization of two documents with two rounds of exchanges.
   */
  // Initialize document A
  const docA = new LoroDoc();
  const listA: LoroList = docA.getList('list');
  listA.insert(0, 'A');
  listA.insert(1, 'B');
  listA.insert(2, 'C');

  // Export the state of document A as a byte array
  const bytes: Uint8Array = docA.export({ mode: 'update' });

  // Simulate sending `bytes` across the network to another peer, B
  const docB = new LoroDoc();
  // Peer B imports the updates from A
  docB.import(bytes);

  // Verify that B's state matches A's state
  expect(docB.toJSON()).toStrictEqual({
    list: ['A', 'B', 'C'],
  });

  // Get the current operation log version of document B
  const version = docB.oplogVersion();

  // Simulate editing at B: delete item 'B'
  const listB: LoroList = docB.getList('list');
  listB.delete(1, 1);

  // Export the updates from B since the last synchronization point
  const bytesB: Uint8Array = docB.export({ mode: 'update', from: version });

  // Simulate sending `bytesB` back across the network to A
  // A imports the updates from B
  docA.import(bytesB);

  // Verify that the list at A now matches the list at B after merging
  expect(docA.toJSON()).toStrictEqual({
    list: ['A', 'C'],
  });
});
```

# Credits

Loro draws inspiration from the innovative work of the following projects and individuals:

- [Ink & Switch](https://inkandswitch.com/): The principles of Local-first Software have greatly influenced this project. The [Peritext](https://www.inkandswitch.com/peritext/) project has also shaped our approach to rich text CRDTs.
- [Diamond-types](https://github.com/josephg/diamond-types): The [Event Graph Walker (Eg-walker)](https://loro.dev/docs/advanced/event_graph_walker) algorithm from @josephg has been adapted to reduce the computation and space usage of CRDTs.
- [Automerge](https://github.com/automerge/automerge): Their use of columnar encoding for CRDTs has informed our strategies for efficient data encoding.
- [Yjs](https://github.com/yjs/yjs): We have incorporated a similar algorithm for effectively merging collaborative editing operations, thanks to their pioneering works.
- [Matthew Weidner](https://mattweidner.com/): His work on the [Fugue](https://arxiv.org/abs/2305.00583) algorithm has been invaluable, enhancing our text editing capabilities.
- [Martin Kleppmann](https://martin.kleppmann.com/): His work on CRDTs has significantly influenced our comprehension of the field.
 

[local-first]: https://www.inkandswitch.com/local-first/
[Fugue]: https://arxiv.org/abs/2305.00583
[Peritext]: https://www.inkandswitch.com/peritext/
