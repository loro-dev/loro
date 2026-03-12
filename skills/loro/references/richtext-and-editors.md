# Rich Text And Editors

## Rich Text Rules

- Use `LoroText` for collaboratively edited text.
- Keep rich text style config in sync across peers:
  - `after`
  - `before`
  - `none`
  - `both`
- If two peers use different `configTextStyle(...)`, they can interpret the same boundary insert differently.

## Text API Notes

- `update(...)` rewrites based on a target text snapshot.
- `updateByLine(...)` is useful when line-oriented reconciliation is more appropriate than raw whole-string replacement.
- `toDelta()` preserves marks and annotations.
- `toJSON()` / `toString()` return plain text only.

## `applyDelta(...)` Caveat

- `applyDelta(...)` is ideal for editor bindings because it matches event diffs.
- Delta inserts must include the full attribute set of the inserted range.
- If CRDT inheritance would add marks but the delta omits them, `applyDelta(...)` removes those attributes.
- Out-of-range formatting can cause Loro to materialize newlines to satisfy editor assumptions.

## Stable Selections

- Use `text.getCursor(...)` to create stable positions.
- Use two cursors for selections: anchor and head.
- Resolve live offsets with `doc.getCursorPos(...)`.
- Persist updated cursors returned from resolution to reduce replay cost.
- WASM text offsets are UTF-16 indices.

## ProseMirror And Tiptap

- Start with `loro-prosemirror` for ProseMirror and Tiptap.
- It already covers document sync, collaborative undo and redo, and cursor presence.
- The current baseline is `CursorEphemeralStore` plus `LoroEphemeralCursorPlugin`.
- Give each editor instance its own `containerId` when several editors share one `LoroDoc`.
- Reuse the same container only if several views intentionally co-edit the exact same content.

## CodeMirror

- Use the official CodeMirror integration when the editor is CM6-based.
- Keep awareness/presence, undo, and document sync at the Loro layer rather than inventing a separate editor-level protocol.

## Rich Text Internals

- Loro rich text uses style anchors to represent mark boundaries.
- Overlap and expansion behavior are first-class concerns.
- Overlappable styles are often best modeled by unique keys with a shared prefix pattern such as `comment:alice` and `comment:bob`.

## Guardrails

- Do not model editable prose as `string` in a `LoroMap`.
- Do not store collaborative selections as plain indices.
- Do not mix old and new ProseMirror cursor APIs accidentally; inspect the package version and current codebase first.
