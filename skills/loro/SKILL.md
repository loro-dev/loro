---
name: loro
description: "Practical guide for helping app developers evaluate, adopt, and use Loro for local-first collaboration. Use when Codex needs to answer questions or write examples for `loro-crdt`, `loro`, `loro-prosemirror`, `loro-codemirror`, `loro-mirror`, language bindings such as Swift/Python/C#/Go/React Native, or Loro-powered apps about: (1) Deciding whether Loro fits a product, (2) Getting started in JavaScript/TypeScript, Rust, or another supported binding, (3) Choosing CRDT containers and document structure, (4) Designing sync, persistence, snapshots, versioning, undo, presence, or time travel, (5) Integrating rich-text editors and stable selections, (6) Mirroring app state with React, or (7) Understanding performance and tradeoffs."
---

# Loro

Use this skill to help users build applications with Loro. Start from the user's product goal, then choose the narrowest chapter that answers it. Load a second chapter only when the task crosses domains.

## Select A Chapter

- Read [references/topic-map.md](references/topic-map.md) if the task is broad and you need to route it.
- Read [references/fit-and-architecture.md](references/fit-and-architecture.md) for product fit, installation, language/package choices, first examples, and core mental models.
- Read [references/containers-and-encoding.md](references/containers-and-encoding.md) for choosing containers, shaping documents, exporting updates, snapshots, persistence, shallow snapshots, and redaction.
- Read [references/sync-versioning-and-events.md](references/sync-versioning-and-events.md) for realtime/offline sync, version vectors, frontiers, checkout, undo, presence, timestamps, events, and Inspector.
- Read [references/richtext-and-editors.md](references/richtext-and-editors.md) for `LoroText`, cursors, `applyDelta`, `updateByLine`, `loro-prosemirror`, Tiptap, and CodeMirror.
- Read [references/mirror-and-react.md](references/mirror-and-react.md) for `loro-mirror`, `$cid`, `idSelector`, validation, selectors, and React integration.
- Read [references/performance-and-tradeoffs.md](references/performance-and-tradeoffs.md) for scaling, document/update size, loading speed, and workload tradeoffs.

## Route Common Tasks

- “I am new to Loro / should I use it / show me a first example”
  - Start with `fit-and-architecture.md`
- “Which language package or binding should I use?”
  - Start with `fit-and-architecture.md`
- “Build a collaborative document model / choose data types / persist history”
  - Start with `containers-and-encoding.md`
  - Add `sync-versioning-and-events.md` if version/history behavior matters
- “Sync devices, store data, use snapshots, time travel, undo, or presence”
  - Start with `sync-versioning-and-events.md`
- “Integrate ProseMirror, Tiptap, CodeMirror, or custom rich text”
  - Start with `richtext-and-editors.md`
  - Add `sync-versioning-and-events.md` if undo/version/event behavior matters
- “Model app state with loro-mirror or loro-mirror-react”
  - Start with `mirror-and-react.md`
  - Add `containers-and-encoding.md` if schema semantics depend on container choice
- “Explain performance, large documents, loading, update size, or tradeoffs”
  - Start with `performance-and-tradeoffs.md`
- “Decide whether Loro is the right tool”
  - Start with `fit-and-architecture.md`
  - Add `performance-and-tradeoffs.md` if workload scale matters

## Execute The Task

1. Classify the task before reading everything.
2. Load one primary chapter.
3. Load at most one secondary chapter for cross-domain work.
4. Keep solutions grounded in Loro semantics:
   - choose data types by merge behavior,
   - distinguish state version from history version,
   - keep ephemeral state out of persisted CRDT data,
   - use stable cursors for collaborative selections,
   - design sync around updates and snapshots rather than a central locking protocol.

## Keep Guardrails

- Do not assume CRDTs are the right fit for hard invariants, exclusivity, or authorization-at-write-time problems.
- Do not model editable text as plain strings when user intent requires merged edits.
- Do not reuse peer IDs across concurrent sessions.
- Do not confuse detached documents with detached containers.
- Do not persist cursor presence, hover state, or connection state inside the main document; use ephemeral state.
