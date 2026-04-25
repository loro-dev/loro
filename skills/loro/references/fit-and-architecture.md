# Getting Started And Fit

## When Loro Fits

- Collaborative text and structured documents.
- Offline-first applications that later synchronize.
- Multi-device sync where eventual consistency is acceptable.
- Apps that benefit from complete history, time travel, or version checkpoints.

## When Loro Does Not Fit By Itself

- Financial or accounting invariants.
- Exclusive ownership or booking/locking semantics.
- Authorization decisions that must be enforced at write time.
- Arbitrary graph-shaped or non-JSON-like data without an adaptation layer.

## Install

- JavaScript/TypeScript: install `loro-crdt`.
- Rust: install the `loro` crate.
- Use editor integrations when available instead of writing editor sync from scratch:
  - `loro-prosemirror` for ProseMirror and Tiptap-style editors.
  - `loro-codemirror` for CodeMirror 6.
- Use `loro-mirror` and `loro-mirror-react` when the app already thinks in immutable state and actions.

## Language And Package Index

- JavaScript/TypeScript: [`loro-crdt`](https://www.npmjs.com/package/loro-crdt). Start here for web apps and most examples in the Loro docs.
- Rust: [`loro`](https://crates.io/crates/loro) and [`docs.rs/loro`](https://docs.rs/loro). Use for native Rust apps, services, CLIs, and custom infrastructure.
- Swift: [`loro-swift`](https://github.com/loro-dev/loro-swift). Use for Swift/iOS/macOS experiments and native apps.
- Python: [`loro-py`](https://github.com/loro-dev/loro-py). Use for Python apps, scripts, and server-side tooling.
- React Native: [`loro-react-native`](https://github.com/loro-dev/loro-react-native). Use for mobile apps that need native React Native bindings.
- C#: [`loro-cs`](https://github.com/sensslen/loro-cs). Community-maintained .NET/C# binding; check its README and release state before adopting.
- Go: [`loro-go`](https://github.com/aholstenson/loro-go). Community-maintained Go binding; check platform support and API completeness before adopting.
- Cross-language FFI base: [`loro-ffi`](https://github.com/loro-dev/loro-ffi). Use this as the reference for generated/native bindings and as a starting point for new language bindings.

Prefer the official JS/TS or Rust package when there is no product reason to choose another language. For community bindings, verify platform binaries, Loro core version, and API coverage before making product commitments.

## First JavaScript Example

```ts
import { LoroDoc } from "loro-crdt";

const docA = new LoroDoc();
const listA = docA.getList("items");
listA.insert(0, "A");
listA.insert(1, "B");

const update = docA.export({ mode: "update" });

const docB = new LoroDoc();
docB.import(update);

console.log(docB.toJSON()); // { items: ["A", "B"] }
```

## First Design Questions

1. What parts of the state should merge when edited concurrently?
2. Which data needs durable history, and which data is only presence or UI state?
3. Do users need text/rich-text intent preservation, ordered moves, tree moves, or simple last-write-wins fields?
4. How will peers exchange updates, and where will snapshots or updates be persisted?

## Conceptual Model

- Loro is a CRDT framework for local-first apps.
- It trades strict coordination for strong eventual consistency.
- Each peer edits locally, exports updates, imports updates from other peers, and converges after seeing the same operations.
- Loro records history, so versioning and time travel are core concepts rather than an afterthought.

## Document Shape Constraints

- Loro documents are JSON-like.
- Map keys are strings.
- The composed document structure is tree-shaped, not a general graph with shared children.

## Entry Points

- Core packages and language bindings are listed in `Language And Package Index`.
- Ecosystem integrations include editor bindings, state-mirroring layers, React Native, and Inspector.

## Practical Start Path

1. Pick container types in `containers-and-encoding.md`.
2. Decide sync and persistence shape in `sync-versioning-and-events.md`.
3. Add editor or React integration only after the document model is clear.
4. Use Inspector during development to inspect state and history.
