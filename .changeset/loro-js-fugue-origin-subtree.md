---
"loro.js": patch
---

Fix a `loro.js` Text/List convergence bug with concurrent inserts. When an insert was concurrent with a run typed after the same character and with another concurrent element whose origin was further left, `loro.js` put the insert after that element instead of at the end of the run. Replicas could disagree with each other (depending on the order in which they received the updates) and with `loro-crdt`. `loro.js` now places these inserts where Rust does.
