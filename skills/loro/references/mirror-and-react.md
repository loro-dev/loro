# Mirror And React

## Mirror Intent

- `loro-mirror` keeps an immutable app-state view in sync with a `LoroDoc`.
- Local `setState(...)` edits become granular CRDT operations.
- Remote CRDT events patch the mirrored app state back in.
- The common mental model is still “immutable app state plus typed actions”, not direct mutation of raw CRDT containers everywhere.

## Root Schema

- Build the root with `schema({...})`.
- Keep root fields as Loro containers: `LoroMap`, `LoroList`, `LoroMovableList`, `LoroText`.
- Put primitives inside those containers, not at the root.

## Field Types

- `schema.String`, `schema.Number`, `schema.Boolean`: concrete primitive fields.
- `schema.Any`: dynamic JSON-like payload when the shape is truly unknown.
- `schema.Ignore`: local-only or derived fields that should not sync to Loro.

## Maps, Lists, And `$cid`

- `schema.LoroMap({...})`: nested object container.
- `schema.LoroMap(...).catchall(valueSchema)`: add dynamic keys while preserving known keys.
- `schema.LoroMapRecord(valueSchema)`: homogeneous record-like map.
- Every mirrored `LoroMap` includes a read-only `$cid`.
- `$cid` matches the underlying Loro container ID.
- Use `$cid` as a stable key or default `idSelector`, especially for list items.
- Never try to persist or mutate `$cid` from app code.
- Do not invent a `withCid`-style option; mirrored map values already receive `$cid` automatically.

## Lists

- `LoroList(itemSchema, idSelector?)`: ordered collection. Add `idSelector` for stable add/remove/update/move diffs.
- `LoroMovableList(itemSchema, idSelector)`: native move semantics, ideal for drag-and-drop.
- If a list item is a map, `(item) => item.$cid` is often the right `idSelector`.

## Defaults And Validation

- Explicit `defaultValue` wins.
- Required fields without explicit defaults fall back to built-in defaults.
- `validateUpdates` is enabled by default in `Mirror`. Keep it on unless a measured hot path proves otherwise.
- Use `validateSchema(...)` when you need an explicit validation pass during migration or debugging.

## React Patterns

- Create a stable `LoroDoc` instance.
- Create helpers through `createLoroContext(schema)` or `useLoroStore(...)`.
- Wrap the tree in `LoroProvider` when using the context pattern.
- Use the narrowest hook that solves the task:
  - `useLoroState` for the full mirrored state
  - `useLoroSelector` for focused subscriptions
  - `useLoroAction` for write paths
- Keep the provider `doc` stable across renders.
- Prefer `useLoroSelector` over pulling the full state into every component.
- Use mirrored list item `$cid` values as React keys.

## Practical Mental Model

- Subscribe to order at collection boundaries.
- Subscribe to content at item boundaries.
- Pass `$cid`s downward and reselect locally instead of passing large mirrored objects through the whole tree.
- Keep view-local state outside the mirror unless collaborators must share it.
