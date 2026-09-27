---
"loro.js": patch
---

Fix `checkout`, `diff`, and undo on a `loro.js` shallow document when a map value or tree node placement was written in the shallow root commit and changed afterwards. When the root commit held more than one op, retreating to the root dropped the map key or tree node instead of restoring its root-time value, and `getLastEditor` returned `undefined`.
