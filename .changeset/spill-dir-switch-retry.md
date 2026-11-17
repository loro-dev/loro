---
"loro-crdt": patch
---

Native `spill_dir`: a failed switch to a new directory no longer records that directory, so setting it again after fixing the problem retries. Add `LoroDoc::try_set_memory_budget` (Rust) to receive the switch error; the switch fills and flushes both new stores before replacing either one, and no longer copies all history entries into an intermediate vector.
