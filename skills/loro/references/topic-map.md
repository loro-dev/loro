# Topic Map

Use this file to choose the right user-facing chapter inside the unified `loro` skill.

## Route By User Ask

- “I am new to Loro. How do I start?”
  - Read `fit-and-architecture.md`
- “Which package, language binding, or platform should I use?”
  - Read `fit-and-architecture.md`
- “Should I use Loro here?”
  - Read `fit-and-architecture.md`
- “Which container should I choose?”
  - Read `containers-and-encoding.md`
- “How should I sync, persist, time-travel, use undo, or debug imports?”
  - Read `sync-versioning-and-events.md`
- “How do I integrate rich text editors?”
  - Read `richtext-and-editors.md`
- “How do I use loro-mirror with React?”
  - Read `mirror-and-react.md`
- “How will Loro behave at scale?”
  - Read `performance-and-tradeoffs.md`

## Getting Started And Product Fit

- Install choices, language bindings, first example, local-first framing
  - Read `fit-and-architecture.md`
- When CRDTs are the wrong fit
  - Read `fit-and-architecture.md`
- Package entry points and ecosystem overview
  - Read `fit-and-architecture.md`

## Containers, Composition, Storage

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
- Undo, cursors, and ephemeral presence
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

## Performance And Tradeoffs

- Loading speed, encoded size, memory, update size
  - Read `performance-and-tradeoffs.md`
- Conflict-heavy vs low-conflict workloads
  - Read `performance-and-tradeoffs.md`
- When to use snapshots, shallow snapshots, or update streams
  - Read `performance-and-tradeoffs.md`
