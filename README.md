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


> ⚠️ **Notice**: The current API and encoding schema of Loro are **experimental** and **subject to change**. You should not use it in production. 

Loro is a [CRDTs(Conflict-free Replicated Data Types)](https://crdt.tech/) library that makes building [local-first apps][local-first] easier. 

Loro is currently available for JavaScript (via WASM) and Rust developers. 

Explore our vision in our blog: [**Reimagine State Management with CRDTs**](https://loro.dev/blog/loro-now-open-source).

# Features

## Supported CRDT Algorithms

- **Common Data Structures**: Includes `List` for ordered collections, LWW (Last Write Win) `Map` for key-value pairs, `Tree` for hierarchical data, and `Text` for rich text manipulation, enabling various applications.
- **Text Editing with Fugue**: Loro integrates [Fugue](https://arxiv.org/abs/2305.00583), a CRDT algorithm designed to minimize interleaving anomalies in concurrent text editing.
- **Peritext-like Rich Text CRDT**: Drawing inspiration from [Peritext](https://www.inkandswitch.com/peritext/), Loro manages rich text CRDTs that excel at merging concurrent rich text style edits, maintaining the original intent of users input as much as possible. Learn more in our blog [Introduction to Loro's Rich Text CRDT](https://loro.dev/blog/loro-richtext).
- **Moveable Tree**: For applications requiring directory-like data manipulation, Loro utilizes the algorithm from [*A Highly-Available Move Operation for Replicated Trees*](https://ieeexplore.ieee.org/document/9563274), which simplifies the process of moving hierarchical data structures.
- [**Moveable List**](https://loro.dev/docs/tutorial/list): Both `List` and `MovableList` utilize Fugue to achieve *maximal noninterleaving*. Additionally, `MovableList` uses the algorithm from [*Moving Elements in List CRDTs*](https://martin.kleppmann.com/2020/04/27/papoc-list-move.html) to implement the move operation.

## Advanced Features in Loro

- **Preserve Editing History**
  - With Loro, you can track changes effortlessly as it records the editing history with low overhead. 
  - This feature is useful for audit trails, undo/redo functionality, and version control.
- **Time Travel Through History**
  - It allows users to compare and merge manually when needed, although CRDTs typically resolve conflicts well.
- **High Performance**
  - [See benchmarks](https://www.loro.dev/docs/performance).

> **Build time travel feature easily for large documents**.


https://github.com/loro-dev/loro/assets/18425020/ec2d20a3-3d8c-4483-a601-b200243c9792


## Features Provided by CRDTs

- **Decentralized Synchronization**: Loro allows your app's state synced via p2p connections.
- **Automatic Merging**: CRDTs guarantee strong eventual consistency by automating the merging of concurrent changes.
- **Local Availability**: Data can be persisted on users' devices, supporting offline functionality and real-time responsiveness. 
- **Scalability**: Effortlessly scale your application horizontally thanks to the inherently distributed nature of CRDTs.
- **Delta Updates**

# Example

[![Open in StackBlitz](https://developer.stackblitz.com/img/open_in_stackblitz.svg)](https://stackblitz.com/edit/loro-basic-test?file=test%2Floro-sync.test.ts)

```ts
import { expect, test } from 'vitest';
import { Loro, LoroList } from 'loro-crdt';

/**
 * Demonstrates synchronization of two documents with two rounds of exchanges.
 */
// Initialize document A
const docA = new Loro();
const listA: LoroList = docA.getList('list');
listA.insert(0, 'A');
listA.insert(1, 'B');
listA.insert(2, 'C');

// Export the state of document A as a byte array
const bytes: Uint8Array = docA.exportFrom();

// Simulate sending `bytes` across the network to another peer, B
const docB = new Loro();
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
const bytesB: Uint8Array = docB.exportFrom(version);

// Simulate sending `bytesB` back across the network to A
// A imports the updates from B
docA.import(bytesB);

// Verify that the list at A now matches the list at B after merging
expect(docA.toJSON()).toStrictEqual({
  list: ['A', 'C'],
});
```

# Credits

Loro draws inspiration from the innovative work of the following projects and individuals:

- [Ink & Switch](https://inkandswitch.com/): The principles of Local-first Software have greatly influenced this project. The [Peritext](https://www.inkandswitch.com/peritext/) project has also shaped our approach to rich text CRDTs.
- [Diamond-types](https://github.com/josephg/diamond-types): The [Replayable Event Graph (REG)](https://loro.dev/docs/advanced/replayable_event_graph) algorithm from @josephg has been adapted to reduce the computation and space usage of CRDTs.
- [Automerge](https://github.com/automerge/automerge): Their use of columnar encoding for CRDTs has informed our strategies for efficient data encoding.
- [Yjs](https://github.com/yjs/yjs): We have incorporated a similar algorithm for effectively merging collaborative editing operations, thanks to their pioneering works.
- [Matthew Weidner](https://mattweidner.com/): His work on the [Fugue](https://arxiv.org/abs/2305.00583) algorithm has been invaluable, enhancing our text editing capabilities.
- [Martin Kleppmann](https://martin.kleppmann.com/): His work on CRDTs has significantly influenced our comprehension of the field.
 

[local-first]: https://www.inkandswitch.com/local-first/
[Fugue]: https://arxiv.org/abs/2305.00583
[Peritext]: https://www.inkandswitch.com/peritext/
