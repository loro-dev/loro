# WASM Bulk Read APIs

Verified against code 2026-09-04

Bulk-read APIs return whole (sub)document values in one WASM call instead of
per-key/per-index accessor round trips.

## APIs

- `LoroDoc.getDeepValueWithID()` (`loro-internal` `DocState::get_deep_value_with_id`):
  root-name → `{ cid, value }` nodes. Child containers inside `value` are
  recursively replaced by their own `{ cid, value }` nodes.
- Per-container `getDeepValueWithID()` on `LoroMap`/`LoroList`/
  `LoroMovableList`/`LoroTree`/`LoroText` (`Handler::get_deep_value_with_id` →
  `DocState::get_container_deep_value_with_id`); same node shape, `cid` is the
  container's own id. Detached containers return an error (JS: throw).
- `LoroList`/`LoroMovableList` `getRangeValue(start, end)` /
  `getRangeDeepValueWithID(start, end)` (`DocState::get_list_range_deep_value`):
  deep-read a `[start, end)` slice; bounds clamp, empty/inverted → `[]`.
- `getDeepValueJson(): string` on `LoroDoc` and all six container classes
  (`DocState::get_deep_value_json`, `*Handler::get_deep_value_json`): JSON text
  of the plain deep value — same content as `JSON.stringify(x.toJSON())`,
  serialized in one WASM call via `serde_json::to_string(&LoroValue)`.
- `getDeepValueJsonWithIds(): { json: string, cids: ContainerID[] }`
  (`DocState::get_deep_value_json_with_ids`, `strip_container_id_nodes` in
  `crates/loro-internal/src/state.rs`): `json` is the deep value WITHOUT ids;
  `cids` is the pre-order DFS of container ids in the serialized tree.

## cid format

`cid` is the bare container id string — exactly what the JS `container.id`
getter returns: `cid:root-<name>:<Type>` for roots, `cid:<counter>@<peer>:<Type>`
for op-created containers (e.g. `cid:root-map:Map`,
`cid:92@2311024965712536503:Map`). Parse the container type from the suffix
after the last `:`.

## cids pre-order contract

`strip_container_id_nodes` converts the with-id `LoroValue` to a
`serde_json::Value` and walks THAT value once, collecting `cids` and building
the stripped JSON in the same pass, iterating each `serde_json::Map` in its own
iteration order. This keeps `cids` consistent with the key/item order a JS
consumer sees after `JSON.parse(json)`, regardless of serde_json's
`preserve_order` feature (this workspace does not enable it, so emitted JSON
object keys are sorted — deterministic, but different from
`getDeepValueJson()`'s direct `LoroValue` serialization order; only the parsed
content is identical between the two APIs).

Pre-order means: a container's cid is pushed before recursing into its value.
For a container-level call, `cids[0]` is that container's own id. Re-attach
walk (see `crates/loro-wasm/tests/deep_value.test.ts`): root object → every
value is a container (consume the next cid); Map → object entries;
List/MovableList/Tree → arrays; Text → string; Counter → number. A position is
treated as a container only when the next pending cid's type matches the value
shape.

Caveats:

- **Tree node meta is a plain deep value** (`get_meta_value` in
  `crates/loro-internal/src/state/tree_state.rs` resolves meta via
  `get_container_deep_value`, not the with-id variant), so meta map container
  ids never appear in `cids`, and a Tree's `value` subtree contains no
  `{ cid, value }` nodes.
- **Structural ambiguity**: node detection is "object with exactly the keys
  `cid` (a string that parses as `ContainerID`) and `value`". A user map that
  stores such an object as plain data would be mistaken for a container node —
  inherent to the format, accepted. Symmetrically, a consumer-side re-attach
  walk cannot distinguish a plain string/number entry from a Text/Counter
  child without schema knowledge.
- `LoroCounter` has no `getDeepValueWithID()`; its `getDeepValueJson()` /
  `getDeepValueJsonWithIds()` work on detached counters too, mirroring
  `toJSON()`.

## Performance

`crates/loro-wasm/scripts/measure-deep-value-json.cjs` (run via root
`pnpm bench-deep-value-json`) builds a ~70k-container doc (Map 15,632 / List
9,956 / Text 44,463, ~3.9 MB JSON) and compares `toJSON()`,
`getDeepValueWithID()`, `getDeepValueJson() (+ JSON.parse)`, and
`getDeepValueJsonWithIds() (+ parse + re-attach walk)`. The JSON text path is
multiple times faster than structured-cloning the with-id value across the
WASM boundary; see the script output for current numbers.
