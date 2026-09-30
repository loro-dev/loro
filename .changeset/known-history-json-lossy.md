---
"loro-crdt": patch
---

Fix a 1.16.4 regression where binary updates or snapshots containing history relayed through JSON were rejected with `UsedOpID`. Known-history checks tolerate binary-to-byte-list, non-finite-number-to-null and integral-Double-to-I64 conversions, including nested values and rich-text styles. Marker-looking strings that JSON turns into Containers and opaque unknown-op payloads retain a value-comparison bypass. The receiver keeps its existing prefix values.

The JSON format is unchanged: a Binary value still comes back from JSON as a list, including mergeable container markers; this fix does not restore values lost during JSON import. JSON imports now compare other representable value payloads rather than skipping all value comparisons, so genuinely different values may be rejected where they were previously silently accepted. Real peer-id conflicts that look like the permitted conversions or use the bypassed payload kinds remain indistinguishable and can still pass.
