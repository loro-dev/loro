---
"loro-crdt": patch
---

Importing updates or a snapshot that reuse op ids the document already has for different content (two clients that shared a peer id) now throws a "ID ... has been used" error and leaves the document unchanged and usable. Previously the import trapped the WASM instance (`RuntimeError: unreachable`) or silently scrambled text. Imports with a change that skips counters of its peer also throw instead of trapping.
