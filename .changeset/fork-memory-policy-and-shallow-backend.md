---
"loro-crdt": patch
---

Preserve independent document memory budgets across fork and forkAt. Use the configured KV backend for shallow snapshot root state when spilling was enabled before import.
