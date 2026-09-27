---
"loro.js": patch
---

Importing a MovableList move or set that names an unknown element, an element of another list, an element outside the op's causal history, or (on a shallow document) an element deleted before the shallow root now fails and rolls back the whole import, as in Rust (loro-dev/loro#1125). A move or set of a deleted element is applied like a concurrent one on every import path.
