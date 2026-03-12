# Fit And Architecture

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

## Conceptual Model

- Loro is a CRDT framework for local-first apps.
- It trades strict coordination for strong eventual consistency.
- Under the hood it mixes Fugue-style correctness with Eg-Walker-inspired replay and simple local indexing.

## Document Shape Constraints

- Loro documents are JSON-like.
- Map keys are strings.
- The composed document structure is tree-shaped, not a general graph with shared children.

## Entry Points

- JS/TS: `loro-crdt`
- Rust: `loro`
- Swift: `loro-swift`
- Python: `loro-py`
- Ecosystem integrations include editor bindings, state-mirroring layers, and Inspector.

## Setup Notes

- In JS frontends, install `loro-crdt`.
- In Vite-based apps, WASM support and top-level-await handling must be configured correctly.
- Inspector is the quickest interactive tool for browsing state and history during development.

## Read Order Inside This Skill

1. Start here for fit and high-level tradeoffs.
2. Switch to `containers-and-encoding.md` for concrete data types and storage choices.
3. Switch to `sync-versioning-and-events.md` for history, events, and sync state.
4. Switch to `richtext-and-editors.md` or `mirror-and-react.md` for integrations.
