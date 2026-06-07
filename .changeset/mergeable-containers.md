---
"loro-crdt": minor
"loro-crdt-map": minor
---

Add mergeable child containers: child containers created under a map key that converge across peers on concurrent first-write instead of forking. Exposed as `ensureMergeable{Counter,Map,List,MovableList,Text,Tree}` on `LoroMap`. A mergeable child lives at a deterministic `ContainerID` derived from `(parent, key, kind)`, and its visibility is driven by a binary ref the parent map stores at the key, so deletes and kind conflicts resolve through the map's regular LWW.
