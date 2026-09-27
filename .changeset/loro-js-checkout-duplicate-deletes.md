---
"loro.js": patch
---

Fix `loro.js` checkout events for text that several peers deleted concurrently. A checkout that restored such text emitted it once per delete (`bccb` instead of `bc`), could split a surrogate pair in the event, and a checkout that also removed such text could throw `event diff position N is out of range`. The document state was already correct; the events now match `loro-crdt`.
