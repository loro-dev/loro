---
"loro-crdt": patch
---

Fix a panic followed by a deadlock when subscribing to an event source while
it is emitting, e.g. calling `subscribeLocalUpdates` or `subscribe` from inside
a callback, or subscribing on one thread while another thread commits or
imports. A subscriber added during an emit is now registered for later events;
it does not receive the event that is already being delivered.
