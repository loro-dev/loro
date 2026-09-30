---
"loro-crdt": patch
---

Fix a 1.16.4 regression where binary updates or snapshots containing history relayed through JSON were rejected with `UsedOpID`. Known-history checks now tolerate JSON's binary-to-byte-list and non-finite-number-to-null conversions, including nested values and rich-text styles, while still rejecting genuine conflicts. The receiver keeps its existing prefix values. The JSON format is unchanged: a Binary value still comes back from JSON as a list, including mergeable container markers; this fix does not restore values lost during JSON import.
