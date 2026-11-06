---
"loro-crdt": patch
---

Stop caller input from panicking or silently losing data. `decodeFrontiers` keeps the greatest id of a peer when the bytes repeat a peer, instead of producing frontiers that trap the next comparison. In detached editing, writing with a peer that already has ops after the current version throws `ConcurrentOpsWithSamePeerID` from the write instead of trapping, and the doc stays usable. `LoroText.update` and `updateByLine` throw the edit's error, such as a deleted container, instead of trapping. `import` now throws `UsedOpID` when the bytes bring no new ops but their content differs from the local ops with the same ids; re-importing history the doc already has is still a no-op.
