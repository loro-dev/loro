---
"loro-crdt": minor
---

Add streaming JSON deep reads on LoroDoc and all container classes. getDeepValueJson()
returns plain JSON; getDeepValueJsonWithIds() returns {json, cids, containerPositions},
where containerPositions is an owned Uint32Array indexing every JSON value in
JavaScript pre-order traversal. This distinguishes scalars from containers, preserves
ordinary cid/value objects and handles numeric object keys correctly. The unmerged
preview's shape-based {json,cids} reconstruction must be replaced by position lookup.
Tree metadata remains plain deep data, matching getDeepValueWithID. Binary values
serialize as JSON arrays; document reads obey empty/deleted-root visibility.
