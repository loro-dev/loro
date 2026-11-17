---
"loro-crdt": patch
---

Global memory budget: when a soft reclamation sweep cannot get below its target, later commits skip the sweep (which compacts history, flushes storage and copies the string arena of the active document) until usage grows by an eighth (at least 256 KiB) or 64 checks were skipped. Hard admission still runs on every commit. Native estimates also scan the registry once instead of twice per sweep.
