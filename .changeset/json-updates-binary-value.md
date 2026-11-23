---
"loro-crdt": patch
---

**JSON updates format change for binary values.** `exportJsonUpdates` (and `get_uncommitted_ops_as_json` / `redactJsonUpdates` output) now encodes `Uint8Array` / `Binary` values, including mergeable container markers and binary values nested in lists and maps, as the string `"🦜:binary:<base64>"` (canonical RFC 4648 Base64 with padding) instead of an array of numbers. Binary values now survive an `exportJsonUpdates` → `importJsonUpdates` round trip, so documents synced via JSON updates no longer fail with `UsedOpID` when they later import binary updates that overlap the same operation IDs.

Compatibility when mixing versions:

- **New reader, old data:** `importJsonUpdates` still accepts JSON updates written by earlier versions, where binary values are arrays of numbers, and also accepts raw `Uint8Array` instances in JavaScript `JsonSchema` objects.
- **Old reader, new data:** `loro-crdt` / `loro` 1.18 and earlier reject JSON updates that contain `"🦜:binary:..."` values with an `Invalid container id` error; the whole import fails and the document is left unchanged. `loro.js` 0.4.1 and earlier import such a value as a plain string. Upgrade every peer that exchanges JSON updates before relying on binary values in them, or keep syncing through the binary `export` / `import` formats, which are unchanged.
- **Reserved prefix:** when JSON updates or a `LoroValue` are deserialized from JSON, a string that starts with `"🦜:binary:"` is now read as a binary value, and is rejected if the rest is not valid Base64. As with the existing `"🦜:"` container id prefix, do not store application strings that begin with this prefix if they go through JSON updates.
