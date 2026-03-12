# Containers And Encoding

## Container Choice

- `LoroMap`: LWW key-value storage. Good for object-like state, coordinates, metadata, URLs, and fields where overwriting is preferable to merging.
- `LoroList`: ordered sequence with insert/delete semantics.
- `LoroMovableList`: ordered sequence with native move and set semantics. Prefer for drag-and-drop or reorder-heavy UIs.
- `LoroTree`: hierarchical moves plus node-local data maps. Prefer for outlines, layers, nested blocks, or parent-child structures.
- `LoroCounter`: additive numeric CRDT. Use when concurrent increments/decrements must accumulate.
- `LoroText`: collaborative text. Use `updateByLine(...)` when line-oriented reconciliation is preferable to raw whole-string replacement.

## Behavioral Notes

- `LoroMap` compares concurrent writes by logical time and retains the winning value.
- Setting a map entry to the same value is a no-op, so it does not create history.
- `LoroList` and `LoroMovableList` both support stable cursors; use them when positions must survive concurrent edits.

## Choice Pitfalls

- Do not store editable prose as a plain string in a map unless LWW semantics are desired.
- Do not model coordinates as `[x, y]` in a list. Concurrent delete+insert can create invalid arrays. Use a map.
- Do not model arbitrary graphs directly. Loro documents compose as trees, not DAGs with shared children.

## Container States And Composition

- Detached containers come from constructors like `new LoroMap()`.
- Attached containers belong to a document and have stable `ContainerID`s.
- Adding a detached container to a document returns an attached version; the original object remains detached.
- Root containers come from `doc.getMap(...)`, `doc.getText(...)`, `doc.getList(...)`, `doc.getTree(...)`, and so on.
- Use `setContainer(...)` and `insertContainer(...)` for nesting, not plain `set(...)` or `insert(...)`.

## Container ID And Overwrite Hazards

- Root container IDs derive from root name plus type.
- Child container IDs derive from the operation that created them.
- Avoid concurrent creation of different child containers under the same map key. Container IDs differ, so one branch can appear overwritten.

## Tree Specifics

- `LoroTree` gives each node an associated `Map` container via `node.data`.
- Fractional indices order siblings. Use them when child ordering matters.
- When many peers insert at the same sibling position, peer ID acts as a tiebreaker for equal fractional indices.
- Fractional index jitter reduces collisions at some encoding-size cost.
- `getNodes(...)` and node JSON views are useful when a flat inspection of the forest is easier than recursive traversal.

## List Vs Movable List

- Use `LoroList` when delete+insert is acceptable and native move identity is unnecessary.
- Use `LoroMovableList` for kanban columns, cards, playlist reordering, or any UX where “move this item” is semantically distinct from delete+insert.

## Counter

- `Counter` aggregates applied values from all peers.
- It is the right choice for additive metrics, not for values with hard invariants.

## Export Modes

- `update`: sync delta payloads.
- `updates-in-range`: bounded history export.
- `snapshot`: full state checkpoint.
- `shallow-snapshot`: current state plus truncated history.

## Import Choices

- `import(...)`: one payload at a time.
- `importBatch(...)`: preferred for multiple updates or mixed snapshot/update payloads because diffing and event emission are coalesced.
- Imports can succeed partially when dependencies are missing. Pending ranges mean the document knows about those operations but cannot apply them yet.

## Persistence Pattern

1. Periodically save a snapshot.
2. Frequently persist updates, often after each local edit or on a debounce.
3. On load, import the snapshot and outstanding updates.
4. Recompact by exporting a fresh snapshot and deleting replayed updates.

## Shallow Snapshots And Redaction

- Use shallow snapshots when old history can be archived or discarded.
- They are often much smaller than full snapshots.
- Peers can only sync if they have versions after the shallow start point.
- Archive full history before trimming if old history may still matter.
- Redaction replaces sensitive payloads while preserving enough structure for future synchronization.
- If old peers still hold unsanitized history, sensitive data still exists there.
