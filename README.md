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


https://github.com/loro-dev/loro/assets/18425020/fe246c47-a120-44b3-91d4-1e7232a5b4ac


> ⚠️ **Notice**: The current API and encoding schema of Loro are **experimental** and **subject to change**. You should not use it in production.

Loro is a CRDTs(Conflict-free Replicated Data Types) library that makes building [local-first apps][local-first] easier. 

Explore our vision for the local-first development paradigm in our blog post: [**Reimagine State Management with CRDTs**](https://loro.dev/blog/loro-now-open-source).

# Features

## Supported CRDT Algorithms

- **Common Data Structures**: Support for `List` for ordered collections, LWW(Last Write Win) `Map` for key-value pairs, `Tree` for hierarchical data, and `Text` for rich text manipulation, enabling various applications.
- **Text Editing with Fugue**: Loro integrates [Fugue](https://arxiv.org/abs/2305.00583), a CRDT algorithm designed to minimize interleaving anomalies in concurrent text editing.
- **Peritext-like Rich Text CRDT**: Drawing inspiration from [Peritext](https://www.inkandswitch.com/peritext/), Loro manages rich text CRDTs that excel at merging concurrent rich text style edits, maintaining the original intent of users input as much as possible. Details on this will be explored further in an upcoming blog post.
- **Moveable Tree**: For applications requiring directory-like data manipulation, Loro utilizes the algorithm from [*A Highly-Available Move Operation for Replicated Trees*](https://ieeexplore.ieee.org/document/9563274), which simplifies the process of moving hierarchical data structures.
- [**Moveable List**](https://loro.dev/docs/tutorial/list): Both `List` and `MovableList` utilize the [*Fugue*](https://arxiv.org/abs/2305.00583) to achieve *maximal noninterleaving*. Additionally, `MovableList` uses the algorithm from [*Moving Elements in List CRDTs*](https://martin.kleppmann.com/2020/04/27/papoc-list-move.html) to implement the move operation.

## Advanced Features in Loro

- **Preserve Editing History**
  - With Loro, you can track changes effortlessly as it records the editing history with low overhead. 
  - This feature is essential for audit trails, undo/redo functionality, and understanding the evolution of your data over time.
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

# Development

### Development Environment Setup

1. **Rust**: Install from the official Rust website.
2. **Deno**: Download and install from Deno's website.
3. **Node**: Install from the Node.js website.
4. **pnpm**: Run `npm i -g pnpm` for global installation.
5. **Rust Target**: Add with `rustup target add wasm32-unknown-unknown`.
6. **wasm-bindgen-cli**: Install version 0.2.90 via `cargo install wasm-bindgen-cli --version 0.2.90`.
6. **wasm-opt**: Install using `cargo install wasm-opt --locked`.
7. **wasm-snip**: Install using `cargo install wasm-snip`.
8. **cargo-nextest**: Install using `cargo install cargo-nextest --locked`.
9. **cargo-fuzz**: Run `cargo install cargo-fuzz`.
10. **cargo-llvm-cov**(to generate coverage report): Run `cargo install cargo-llvm-cov` 

### Test

```bash
deno task test

# Build and test WASM
deno task test-wasm
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
