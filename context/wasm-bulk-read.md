# WASM bulk reads and the performance stack

Verified against code 2026-09-05.

## Which cost each change removes

- #1093 bounds the decoded-value cache used by individual handles. This fixes
  retained WASM memory growth even for callers that keep their existing reads.
- #1085 caches wrapper `kind()`; #1086 adds subtree/range deep reads. These
  reduce per-field JS/WASM calls. A visible history window should use range
  reads rather than read an entire document just because a bulk API exists.
- #1087 serializes deep values in Rust and returns JSON text. Both JSON APIs
  stream ephemeral container values into the output, without first building a
  deep `LoroValue` tree or a `serde_json::Value` tree. The ID variant adds a
  sparse position index. Tree metadata still uses its existing deep-value
  conversion; the streaming improvement primarily targets Map/List/Text reads. Consumers can build their projection/registry in one
  JS walk, without fetching each container again.
- #1090 defines the causal boundary for shallow snapshots. #1091 reduces the
  cost of constructing their root state, with replay cost limits. These are
  history/bootstrap/export changes, independent of the JSON read format.

A faster bulk read does not automatically accelerate `new Mirror`: its caller
must adopt it while preserving schema decoding, ignored fields, container
registration, tree normalization and subscription behavior. The benchmark's
projection/registry cases model that read work, not the full Mirror constructor.

## APIs

- `getDeepValueWithID()` on documents and Map/List/MovableList/Tree/Text returns
  `{ cid, value }` nodes for containers, with bare `ContainerID` strings.
- List/MovableList `getRangeDeepValueWithID(start, end)` and
  `getRangeValue(start, end)` read a clamped `[start, end)` slice in one call.
- `getDeepValueJson(): string` returns the plain deep value as JSON text.
- `getDeepValueJsonWithIds(): DeepValueJsonWithIds` returns:

```ts
type DeepValueJsonWithIds = {
  json: string;
  cids: ContainerID[];
  containerPositions: Uint32Array;
};
```

These JSON APIs exist on the document and all six container classes. Detached
containers throw, except Counter, which supports detached value reads. JSON
follows Rust's JSON serialization of the deep value: binary becomes an array,
non-finite numbers become null, and arbitrary plain objects stay ordinary data.
Document JSON obeys empty/deleted-root visibility. The older structured
`getDeepValueWithID()` does not apply those display filters.

## Sparse position contract

`cids[i]` identifies the value at `containerPositions[i]` in a **pre-order walk
of every value** in `JSON.parse(json)`:

- Start at zero. Count each object, array and scalar once; do not count keys.
- Visit arrays by increasing index and objects in `Object.keys()` order.
- Binary is a JSON array: its byte values count too.
- A document's root object counts as zero, but is not a container.
- A container-level result marks position zero with its own cid.
- Tree metadata is plain deep data, matching the existing with-id API: count
  those JSON values, but do not assign them additional cids.
- Positions increase strictly and have the same length as `cids`. The typed
  array owns a copied buffer and survives subsequent WASM calls and doc.free().

For example, these values are distinct even when both fields contain `"same"`:

```text
JSON: {"m":{"a":"same","b":"same"}}
walk:  0    1    2          3

Text at a: cids = [mapId, textId], positions = [1, 2]
Text at b: cids = [mapId, textId], positions = [1, 3]
```

The writer discovers identity from actual container edges, including mergeable
markers at map edges. It never strips objects that happen to contain `cid` and
`value`, nor guesses Text/Counter identity from a scalar type. It orders integer
property keys as JavaScript does (`"2"` before `"10"`; `"01"` and `"4294967295"`
are ordinary keys), independent of serde_json's `preserve_order` feature.

A consumer can wrap known positions in `{cid, value}`, stamp map `$cid` fields,
or register identities directly during its own projection walk. Do not repeat
handle lookups to recover identities that are already present in the result.
The tests and benchmark contain complete reattachment examples; mutation of an
existing JSON.parse object preserves own `__proto__` keys without invoking the
inherited prototype setter.

## Why positions instead of full paths

For C containers at average depth D, full paths duplicate O(C*D) key/index
segments and require one path array per container. The sparse sidecar is O(C),
exactly 4*C bytes plus one JS typed-array allocation; it reuses the strings
already present in JSON. It also handles arbitrary keys without path escaping.

The tradeoff is traversal: positions require visiting all JSON values, including
plain data. Paths allow direct navigation to each container and can have a faster
JS-only attachment step, especially with large plain subtrees. They still cost
path construction, serialization/transfer and extra allocations. Benchmark both
sides of this tradeoff instead of inferring speed from metadata bytes alone.

The previous `{json,cids}` API in the unmerged PR did not record positions and
could not recover mixed primitive/container layouts. Consumers of that preview
must use the new index; the old shape-based reattachment is not compatible.

## Reproducible measurements

Run `pnpm release-wasm`, then `pnpm bench-deep-value-json`. The benchmark uses a
synthetic 70,051-container fixture (15,632 Map / 9,956 List / 44,463 Text) and
validates output before reporting results. No user document is stored.

Each read case runs in a separate process with a newly imported snapshot. It
reports cold time, warm median (2 warm-ups + 5 measured rounds), and external,
heap and RSS deltas while retaining the first result. External memory includes
WASM linear-memory growth and other ArrayBuffers; it is not an exact Rust
allocation peak. The handle/indexed projection cases both stamp map identities
and register all containers. Path/position attachment timings isolate only JS
consumption, excluding path production/transfer; path payload size is also shown.

Set `LORO_BENCH_MODULE=/absolute/path/to/nodejs/index.js` to run the same fixture
against another release build. A baseline without position support runs the
existing API cases only. Plain JSON and identity-preserving reads are separate
contracts and must not be advertised as interchangeable speed comparisons.

## Measurement on this revision

Release WASM, Node 22.23.1, macOS arm64, 2026-09-05; one run of the command above. Timings
are machine-dependent. The two identity-preserving rows reconstruct the same
with-id value; the two projection rows produce the same plain value and cid
registry (without Mirror schema/lifecycle work).

| Read | Cold ms | Warm median ms | External delta MiB |
| --- | ---: | ---: | ---: |
| Structured getDeepValueWithID | 279.5 | 236.4 | 41.19 |
| Indexed JSON + parse + reattach | 248.1 | 204.4 | 19.39 |
| Per-handle projection + registry | 315.5 | 270.3 | 17.88 |
| Indexed JSON projection + registry | 249.1 | 207.2 | 19.13 |
| Plain streaming JSON + parse | 196.1 | 163.2 | 18.00 |

Position metadata: 280,204 bytes. Full-path JSON metadata for the same
containers: 3,584,299 bytes (716,208 repeated key/index segments), excluding
cid strings common to both designs. Prebuilt-sidecar JS parse/attachment took
23.2 ms with positions vs 12.5 ms with paths. The latter excludes generating,
copying and decoding path metadata, so it is not an end-to-end path benchmark.
The position design trades a modest full-value JS walk for a 12.8x smaller
location payload and no per-container path arrays. It is not a claimed 5x
end-to-end speedup.

The same benchmark against the original PR head (`7e99105a`, using
`LORO_BENCH_MODULE`) measured its legacy ID JSON producer at 365.3 ms cold /
234.3 ms warm and 96.06 MiB external growth. The streaming indexed producer
measured 226.8 ms / 174.3 ms and 19.39 MiB. These producer-only rows exclude
parse/reattachment, and the legacy result cannot represent all container layouts;
they are separate from the valid identity-preserving consumer comparison above.
