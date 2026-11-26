---
"loro-crdt": patch
---

Refuse crafted updates whose run-length columns declare far more rows than their bytes, without allocating memory for those rows. The block's delete-start-id column is read lazily, and its tree position arena is decoded row by row (loro-dev/loro#1349). Before, a 195-byte update allocated about 1 GiB and a 207-byte update requested 16 GiB before failing.
