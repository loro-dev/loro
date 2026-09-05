# Structured WASM reads and the performance stack

Verified against code 2026-09-06.

## Responsibility

- #1093 bounds retained decoded container state; it benefits existing handle reads.
- #1085 caches wrapper kind; #1086 supplies legacy subtree/range reads with IDs.
- #1087 adds `LoroDoc.toContainerTree` to construct a consumer-ready JS snapshot directly.
- #1090/#1091 concern shallow snapshot causal boundaries and constructing their
  root state. They optimize history import/export, independently of this read API.

## Contract

```ts
const roots = doc.toContainerTree();
const subtree = map.toContainerTree();
const window = list.toContainerTreeSlice(20, 40);
const formatted = text.toContainerTree({text: "delta"});
```

Containers are `{type, cid, value}`. Map values and List/MovableList items are
nodes. An ordinary value is `{type: "Value", value}`: its contents are opaque,
including plain objects shaped exactly like container nodes. Text contains a
string by default, or its formatting delta when requested. Tree contains the
existing nested tree layout, but each `meta` is an ID-bearing Map node; its
fields follow the same node contract. Counter contains a number.

Identity is emitted only at real CRDT edges, including mergeable Map markers
and Tree metadata edges. No keys, scalar values, paths, traversal positions, or
user object shapes are used to guess identity. Full-document reads obey root
visibility. An attached root handle can read its empty root. Detached handles and unknown container types return errors. Document roots selection uses visible root names; missing names are omitted without creating roots, duplicates are ignored, and an empty selection returns {}. Reads do not
commit; caller mutations of returned objects/buffers do not mutate the document.

List/MovableList.toContainerTreeSlice requires nonnegative u32 integer bounds;
end is exclusive, bounds clamp, inverted ranges are empty. It returns {cid, start, totalLength, items}, not a full container node; metadata and items are read under one state lock. Only selected child
subtrees are traversed, though obtaining the parent shallow list is still O(N).
Read traversal rejects nesting above 256 levels. JS number conversion follows
existing reads (including i64 rounding, NaN and infinity); Binary is Uint8Array.
Own `__proto__` and integer keys are ordinary data properties and do not invoke
inherited setters. Consumers must preserve this when projecting nodes themselves.

## Implementation

`state/read_state.rs` emits a traversal to a sink without constructing a deep
whole-document LoroValue tree. Each container's shallow value is ephemeral;
values fetched to determine root visibility are reused. `loro-wasm/src/read_state.rs`
owns the JS construction stack and uses fixed imported functions for stable
wrapper shapes, IDs and own-property writes. Keys and peer decimal strings are
cached only for one read; complete CIDs are constructed in JS. Binary buffers
are copied once into an owned JS Uint8Array. There is no public transport format,
codec, path index, JS callback, or document-lifetime cache.

Inductive correctness: each ordinary edge emits one opaque Value node preserving
its value; each container edge emits its actual ID and the recursively transformed
children; the document enumerates exactly its visible roots. Thus projecting
wrappers away preserves visible values and walking only node children enumerates
container occurrences without confusing embedded Map/List data. The proof relies
on the existing shallow-value/mergeable-edge semantics. Tests separately check
Tree metadata, deltas, binary ownership, special keys, IDs, ranges, errors and
root visibility; this is a design argument, not a machine-checked proof.

## Mirror integration

Mirror can consume the node tree in its existing schema/registry walk. On a Value
node it decodes the opaque value; on a container node it registers `cid` and
recurses by `type`. This removes shallow reads previously needed to distinguish
legacy `{cid,value}` nodes from user objects. Schema decoding, Ignore fields,
lazy hydration, Tree normalization, and incremental events remain Mirror's work.
A bulk API should not force eager traversal of lazy history: use subtree/range
reads where the schema allows it. Legacy `getDeepValueWithID` remains unchanged.

## Measured end-to-end cost

Final Node 22.23.1 and Chromium 152 comparisons used the same newly built Loro
package and Mirror adapter, an imported local 61.85 MiB snapshot, 10,921 reachable
containers (10,925 after schema-created roots), and the actual application schema.
Each fresh process/page measured its first constructor separately, then two warmups
and five samples; cases ran forward and backward. Import was outside the timed
constructor. Full state and sorted registered IDs agreed across methods. Private
snapshots/transcripts are not fixtures and are not committed.

| Full Mirror initialization | Node warm median | Chromium warm median |
| --- | --- | --- |
| Per-container handles | 64.5–66.3 ms | 58.8–59.4 ms |
| Legacy bulk + shallow disambiguation | 79.4–80.5 ms | 73.0–74.2 ms |
| Fixed constructors + explicit nodes | 51.9–53.1 ms | 47.2–47.3 ms |

First constructor: new path 80–83 ms in Node, 71–73 ms in Chromium; handle path
89–91 ms and 85–88 ms respectively. JS retained heap was similar (about 37 MiB);
reuse of already allocated WASM memory must not be described as zero memory cost.
A synthetic 70,051-container sample measured about 256 ms versus 321 ms for legacy
bulk; such small-string workloads do not predict the ranking of string transports
on the large-text application document.

Moving the entire builder stack to JS did not consistently win and was rejected.
Pre-creating dense array slots likewise gave no material improvement. The shipped
builder retains fixed wrapper constructors, per-read peer/key reuse, and safe own
property writes. These timings describe the tested machines/workload, not a general
speed guarantee. Mirror's Tree normalization still uses its existing handle path.

Document toContainerTree({roots}) filters before reading root values; the optional text format applies recursively. TypeScript infers both the receiver kind and Text value format. Mirror selects schema roots and, when preserving unknown roots, includes them while excluding explicit Ignore roots.
