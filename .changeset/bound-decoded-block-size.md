---
"loro-crdt": patch
---

Refuse change blocks with zero-length ops or ops that do not exactly cover the block's counter range, read delete ids lazily, and stop decompressing a kv-store block once it exceeds the largest size its format allows. Decoding a crafted update or snapshot could otherwise allocate memory far beyond the input size. A `LoroDoc` whose lock a panic poisoned can now be dropped without panicking again.
