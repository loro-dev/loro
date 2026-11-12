---
"loro-crdt": patch
---

Return `ImportUpdatesThatDependsOnOutdatedVersion` instead of panicking when an
import mixes retained shallow-root dependencies with trimmed history, or replays
an obsolete pending change. Keep valid import progress and pending data usable
without poisoning document locks.
