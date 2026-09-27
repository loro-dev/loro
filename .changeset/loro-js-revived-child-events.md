---
"loro.js": patch
---

Fix `loro.js` events for child containers that a checkout or import attaches again, for example a map key set back to an older text, or a restored list element that holds a container. The event for such a child now carries its whole state, like `loro-crdt`, instead of nothing or a delta against the state it had while it was hidden. A listener that mirrors events into its own model (starting an attached child from empty) now ends up with the document's state. Events are also ordered parent first.
