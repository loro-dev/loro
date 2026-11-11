---
"loro-crdt": patch
---

Fix operation-span queries between retained versions on shallow documents without
walking pruned dependencies. Return a history error for versions before the
shallow root, including equal endpoints.
