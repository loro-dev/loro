---
name: loro
description: "Comprehensive guide for using and maintaining Loro across document modeling, synchronization, versioning, rich text editors, app-state mirroring, performance tradeoffs, Rust internals, tests, and wasm bindings. Use when Codex needs to work with `loro-crdt`, `loro`, `loro-internal`, `loro-prosemirror`, `loro-mirror`, or this repository for: (1) Choosing CRDT container types and document structure, (2) Designing sync, persistence, checkout, or history workflows, (3) Integrating rich-text editors and stable selections, (4) Mirroring app state with schemas and React, (5) Reasoning about versions, events, import status, or Inspector output, (6) Maintaining Rust/JS/WASM APIs and tests, or (7) Understanding Loro performance and research tradeoffs."
---

# Loro

Use this skill as the single entry point for all Loro work. Load one primary chapter first. Load a second chapter only when the task clearly crosses domains.

## Select A Chapter

- Read [references/topic-map.md](references/topic-map.md) if the task is broad and you need to route it.
- Read [references/fit-and-architecture.md](references/fit-and-architecture.md) for CRDT fit, local-first framing, setup, and high-level architecture.
- Read [references/containers-and-encoding.md](references/containers-and-encoding.md) for container choice, composition, encoding, persistence, shallow snapshots, and redaction.
- Read [references/sync-versioning-and-events.md](references/sync-versioning-and-events.md) for sync flows, frontiers, version vectors, checkout, import status, timestamps, event timing, and Inspector.
- Read [references/richtext-and-editors.md](references/richtext-and-editors.md) for `LoroText`, cursors, `applyDelta`, `updateByLine`, `loro-prosemirror`, Tiptap, and CodeMirror.
- Read [references/mirror-and-react.md](references/mirror-and-react.md) for `loro-mirror`, `$cid`, `idSelector`, validation, selectors, and React integration.
- Read [references/repo-development.md](references/repo-development.md) for this repository's crate layout, API source files, test placement, validation commands, and compatibility rules.
- Read [references/wasm-maintenance.md](references/wasm-maintenance.md) for `crates/loro-wasm`, `#[wasm_bindgen]`, pending-event flushing, wrapper decoration, and tests.
- Read [references/performance-and-research.md](references/performance-and-research.md) for benchmarks, Eg-Walker tradeoffs, movable-tree context, rich-text design context, and project history.

## Route Common Tasks

- “Build a collaborative document model / choose data types / persist history”
  - Start with `containers-and-encoding.md`
  - Add `sync-versioning-and-events.md` if version/history behavior matters
- “Debug checkout / detached mode / missing imports / event timing”
  - Start with `sync-versioning-and-events.md`
- “Integrate ProseMirror, Tiptap, CodeMirror, or custom rich text”
  - Start with `richtext-and-editors.md`
  - Add `sync-versioning-and-events.md` if undo/version/event behavior matters
- “Model app state with loro-mirror or loro-mirror-react”
  - Start with `mirror-and-react.md`
  - Add `containers-and-encoding.md` if schema semantics depend on container choice
- “Modify Loro source, public APIs, tests, docs, or package output”
  - Start with `repo-development.md`
  - Add `wasm-maintenance.md` when the change touches `crates/loro-wasm`
- “Change wasm bindings or debug pending event flushing”
  - Start with `wasm-maintenance.md`
- “Decide whether Loro is even the right tool / explain tradeoffs”
  - Start with `fit-and-architecture.md`
  - Add `performance-and-research.md` if benchmark or research context matters

## Execute The Task

1. Classify the task before reading everything.
2. Load one primary chapter.
3. Load at most one secondary chapter for cross-domain work.
4. Keep solutions grounded in Loro semantics:
   - choose data types by merge behavior,
   - distinguish state version from history version,
   - keep ephemeral state out of persisted CRDT data,
   - respect public API compatibility and binding/runtime invariants.

## Keep Guardrails

- Do not assume CRDTs are the right fit for hard invariants, exclusivity, or authorization-at-write-time problems.
- Do not model editable text as plain strings when user intent requires merged edits.
- Do not reuse peer IDs across concurrent sessions.
- Do not confuse detached documents with detached containers.
- Do not silently continue after internal invariant violations; invalid external input should become `Result::Err`, but corrupted internal state should fail fast.
- Do not change wasm-exposed mutators without checking pending-event flushing behavior.
