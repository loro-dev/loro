---
"loro-crdt": patch
---

Reject imported updates with a text style end anchor that does not directly follow its style start. Such a malformed update used to panic in the rich-text diff and leave the document unusable; `import` now returns a decode error and leaves the document unchanged.
