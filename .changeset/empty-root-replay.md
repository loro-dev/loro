---
"loro-crdt": patch
---

A root container touched by a document's history is now part of the value after importing that history, even when its ops add up to an empty value (for example text inserted and then deleted). Previously the writing peer showed `{"t": ""}` while a replay of the same history showed `{}`.
