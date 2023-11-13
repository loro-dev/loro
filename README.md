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
  <a aria-label="X" href="https://x.com/loro_dev" target="_blank">
    <img alt="" src="https://img.shields.io/badge/Twitter-black?style=for-the-badge&logo=Twitter">
  </a>
  <a aria-label="Discord-Link" href="https://discord.gg/tUsBSVfqzf" target="_blank">
    <img alt="" src="https://img.shields.io/badge/Discord-black?style=for-the-badge&logo=discord">
  </a>
</p>

![Rich text example](https://github.com/loro-dev/loro/assets/18425020/54b6975d-ea9e-4c77-9423-c7733b12a251)

> **Notice**: The current API and encoding schema of Loro are **experimental** and **subject to change**. 

Loro is a CRDTs(Conflict-free Replicated Data Types) library that makes building [local-first apps][local-first] easier. 

Explore our vision for the local-first development paradigm in our blog post: [**Reimagine State Management with CRDTs**](https://loro.dev/blog/loro-now-open-source).

# Features

## Supported CRDT Algorithms

- **Common Data Structures**: Support for `List` for ordered collections, LWW(Last Write Win) `Map` for key-value pairs, `Tree` for hierarchical data, and `Text` for rich text manipulation, enabling various applications.
- **Text Editing with Fugue**: Loro integrates [Fugue](https://arxiv.org/abs/2305.00583), a CRDT algorithm designed to minimize interleaving anomalies in concurrent text editing.
- **Peritext-like Rich Text CRDT**: Drawing inspiration from [Peritext](https://www.inkandswitch.com/peritext/), Loro manages rich text CRDTs that excel at merging concurrent rich text style edits, maintaining the original intent of users input as much as possible. Details on this will be explored further in an upcoming blog post.
- **Moveable Tree**: For applications requiring directory-like data manipulation, Loro utilizes the algorithm from [*A Highly-Available Move Operation for Replicated Trees*](https://ieeexplore.ieee.org/document/9563274), which simplifies the process of moving hierarchical data structures.

## Advanced Features in Loro

- **Preserve Editing History**
  - With Loro, you can track changes effortlessly as it records the editing history with low overhead. 
  - This feature is essential for audit trails, undo/redo functionality, and understanding the evolution of your data over time.
- **Time Travel Through History**
  - It allows users to compare and merge manually when needed, although CRDTs typically resolve conflicts well.
- **High Performance**
  - [See benchmarks](https://www.loro.dev/docs/performance).

> **Build time travel feature easily for large documents**.

![TimeTravel](https://github.com/loro-dev/loro/assets/18425020/52a34c9d-0d3b-496f-a75d-3c1e01ff139f)


## Features Provided by CRDTs

- **Decentralized Synchronization**: Loro allows your app's state synced via p2p connections.
- **Automatic Merging**: CRDTs guarantee strong eventual consistency by automating the merging of concurrent changes.
- **Local Availability**: Data can be persisted on users' devices, supporting offline functionality and real-time responsiveness. 
- **Scalability**: Effortlessly scale your application horizontally thanks to the inherently distributed nature of CRDTs.
- **Delta Updates**

# Credits

Loro draws inspiration from the innovative work of the following projects and individuals:

- [Ink & Switch](https://inkandswitch.com/): The principles of Local-first Software have greatly influenced this project. The [Peritext](https://www.inkandswitch.com/peritext/) project has also shaped our approach to rich text CRDTs.
- [Diamond-types](https://github.com/josephg/diamond-types): The ingenious OT-like merging algorithm from @josephg has been adapted to reduce the computation and space usage of CRDTs.
- [Automerge](https://github.com/automerge/automerge): Their use of columnar encoding for CRDTs has informed our strategies for efficient data encoding.
- [Yjs](https://github.com/yjs/yjs): We have incorporated a similar algorithm for effectively merging collaborative editing operations, thanks to their pioneering contributions.
- [Matthew Weidner](https://mattweidner.com/): His work on the [Fugue](https://arxiv.org/abs/2305.00583) algorithm has been invaluable, enhancing our text editing capabilities.

 
[local-first]: https://www.inkandswitch.com/local-first/
[Fugue]: https://arxiv.org/abs/2305.00583
[Peritext]: https://www.inkandswitch.com/peritext/
