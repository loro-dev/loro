---
"loro-crdt": patch
---

Fix a 1.16.4 regression where history relayed through JSON was rejected with `UsedOpID`, without letting binary import treat those conversions as identical.

Binary `import`, `import_with`, fast updates, fast snapshots, and `import_batch` stay exact: overlapping op ids must be structurally identical, including finite doubles, large integers, binary versus a list, non-finite doubles versus null, marker strings versus containers, and unknown payloads. JSON import, and `import_with_history_mode` / `import_batch_with_history_mode` with `ImportHistoryMode::JsonLossy`, tolerate the conversions JSON is known to introduce: I64/I64 or I64/Double that match as f64 when at least one integer is outside `[-2^53, 2^53]`; binary versus a same-length list of byte numbers; non-finite doubles versus null; a container-marker string versus any Container; unknown payloads skipped. Finite doubles still use `==`. The receiver keeps its existing prefix values.

Finite doubles round-trip because `serde_json`'s `float_roundtrip` feature is enabled on the normal dependency. Counter deltas are integer-encoded only when the fraction is exactly 0, and a nonzero delta is applied on import even when it is smaller than `f64::EPSILON`. The JSON format is otherwise unchanged, and this does not restore values a JSON importer already dropped. Callers must opt in before a binary blob that carries those degraded values will import.
