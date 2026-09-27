---
"loro.js": patch
---

`loro.js` shallow snapshots now pick the same root as `loro-crdt`: the latest single-head critical version of the requested frontiers and the latest version (loro-dev/loro#1095). Before, `loro.js` used the requested version (or the meet of its heads) even when a retained op was concurrent with it. Checking out retained versions of such a snapshot and returning to the latest version could reorder concurrent Text/List elements, and the snapshot disagreed with a Rust export of the same document.
