---
"loro-crdt": patch
---

Speed up local map/text edits, block encode/decode, linear list diffs, and quiet causal update imports: skip unchanged map writes under the transaction state lock, batch block container registration with single-pass update block decoding, defer `RichtextTracker` allocation on linear diffs, and apply quiet linear causal imports directly to `DocState`.
