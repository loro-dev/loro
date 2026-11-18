---
"loro-crdt": patch
---

Preserve `Uint8Array` / `Binary` values (including mergeable container markers and nested binary payloads) across `exportJsonUpdates` and `importJsonUpdates` by encoding binary values in JSON updates as `"🦜:binary:<base64>"` (canonical RFC 4648 Base64), while also accepting raw `Uint8Array` instances in JavaScript `JsonSchema` objects and keeping legacy number-array JSON imports compatible. Documents synced via JSON updates no longer fail with `UsedOpID` when subsequently importing binary updates that overlap the same operation IDs.
