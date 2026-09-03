---
"loro-crdt": minor
---

Add JSON text export of deep values. `LoroDoc`, `LoroMap`, `LoroList`, `LoroMovableList`, `LoroTree`, `LoroText`, and `LoroCounter` now expose:

- `getDeepValueJson(): string` — the same content as `JSON.stringify(x.toJSON())`, produced inside WASM in a single call, avoiding the cost of crossing the WASM/JS boundary with a large structured value.
- `getDeepValueJsonWithIds(): { json: string, cids: ContainerID[] }` — `json` parses to the same content as `getDeepValueJson()` (the deep value WITHOUT container ids) and `cids` lists the container id strings in pre-order DFS of the serialized JSON tree, so a consumer can re-attach ids in a single JS walk to reconstruct the `getDeepValueWithID()` shape. For a container, `cids[0]` is that container's own id.

Detached containers throw a readable error instead of trapping (except `LoroCounter`, which mirrors `toJSON()` and also works detached). Note: a plain object value that has exactly the keys `cid` and `value` with `cid` being a valid container id string is indistinguishable from a container node in the `cids` format. Tree node meta maps are plain deep values, so meta container ids do not appear in `cids`.
