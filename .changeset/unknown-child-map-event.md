---
"loro-crdt": patch
---

A map event that sets a key to a container of a type unknown to this version (created by a newer `loro-crdt`) is now delivered with its other keys; only the unknown child's entry is left out. Previously the whole map event was dropped, so edits to known keys in the same event never reached subscribers.
