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

- 📖 Preserve Editing History in a [Replayable Event Graph](https://loro.dev/docs/advanced/replayable_event_graph)
- ⏱️ Fast [Time Travel](https://loro.dev/docs/tutorial/time_travel) Through History

https://github.com/loro-dev/loro/assets/18425020/ec2d20a3-3d8c-4483-a601-b200243c9792

# Example

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
