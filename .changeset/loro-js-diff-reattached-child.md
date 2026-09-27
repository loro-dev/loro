---
"loro.js": patch
---

`diff()` and `revertTo()` in `loro.js` now include the full state of a child container the range attaches again without editing it, as Rust does. For example, reverting a map key from a string back to an older child map restored an empty map. The restored map now keeps its content, including nested containers.
