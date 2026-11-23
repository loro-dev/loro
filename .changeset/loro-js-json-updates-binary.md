---
"loro.js": patch
---

Preserve binary values in JSON updates by encoding them as `"🦜:binary:<base64>"`, while continuing to accept legacy number-array JSON imports. This keeps JSON updates interoperable with `loro-crdt` 1.19 and prevents conflicts when JSON updates are followed by overlapping binary updates.
