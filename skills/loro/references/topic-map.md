# Topic Map

Use this file to choose the right chapter inside the unified `loro` skill.

## Route By User Ask

- “Should I use Loro here?”
  - Read `fit-and-architecture.md`
- “Which container should I choose?”
  - Read `containers-and-encoding.md`
- “How should I sync, persist, time-travel, or debug imports?”
  - Read `sync-versioning-and-events.md`
- “How do I integrate rich text editors?”
  - Read `richtext-and-editors.md`
- “How do I use loro-mirror with React?”
  - Read `mirror-and-react.md`
- “How do I change crates/loro-wasm safely?”
  - Read `wasm-maintenance.md`
- “Why does Loro behave this way, and what are the performance tradeoffs?”
  - Read `performance-and-research.md`

## Product Fit And High-Level Architecture

- CRDT basics, CAP tradeoffs, local-first framing, OT comparison
  - Read `fit-and-architecture.md`
- When CRDTs are the wrong fit
  - Read `fit-and-architecture.md`
- Package entry points, setup notes, ecosystem overview
  - Read `fit-and-architecture.md`

## Containers, Composition, Encoding, Persistence

- Choosing between `LoroMap`, `LoroList`, `LoroMovableList`, `LoroTree`, `LoroCounter`, `LoroText`
  - Read `containers-and-encoding.md`
- Composition rules, container states, container IDs, overwrite hazards
  - Read `containers-and-encoding.md`
- Snapshot, update, shallow snapshot, updates-in-range
  - Read `containers-and-encoding.md`
- Persistence, redaction, history trimming
  - Read `containers-and-encoding.md`

## Sync, Versions, History, Events

- Peer sync, update exchange, realtime sync, offline merge
  - Read `sync-versioning-and-events.md`
- Frontiers, version vectors, OpLog vs DocState
  - Read `sync-versioning-and-events.md`
- Checkout, time travel, attached vs detached state
  - Read `sync-versioning-and-events.md`
- Peer IDs, import status, transactions, changes, timestamps
  - Read `sync-versioning-and-events.md`
- Event timing, `subscribePreCommit`, Inspector
  - Read `sync-versioning-and-events.md`

## Text, Rich Text, And Editors

- `LoroText`, marks, delta APIs, `updateByLine`, stable selections
  - Read `richtext-and-editors.md`
- ProseMirror, Tiptap, CodeMirror, custom editor bindings
  - Read `richtext-and-editors.md`
- Rich text internals, style anchors, overlap, expansion rules
  - Read `richtext-and-editors.md`

## App-State Mirroring

- `loro-mirror`, schemas, `$cid`, `idSelector`, validation
  - Read `mirror-and-react.md`
- `loro-mirror-react`, selectors, provider/hooks patterns
  - Read `mirror-and-react.md`

## WASM Binding Maintenance

- `crates/loro-wasm` API changes
  - Read `wasm-maintenance.md`
- Pending-event flushing and JS decorator allowlists
  - Read `wasm-maintenance.md`
- Binding aliases, generated TS docs, wasm tests
  - Read `wasm-maintenance.md`

## Performance And Research Context

- Benchmark interpretation
  - Read `performance-and-research.md`
- Event-graph replay and efficiency tradeoffs
  - Read `performance-and-research.md`
- Movable-tree algorithm context, rich-text algorithm context, project history
  - Read `performance-and-research.md`
