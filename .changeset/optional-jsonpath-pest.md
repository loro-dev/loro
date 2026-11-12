---
"loro-crdt": patch
---

Make the Rust JSONPath parser dependencies optional so builds without the `jsonpath` feature no longer require `pest` or `pest_derive`. Keep the default Rust features and WASM JSONPath support unchanged.
