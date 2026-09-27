---
"loro.js": patch
---

`exportJsonUpdates` in `loro.js` now writes JSON that Rust and `loro-crdt` accept, in their format:

- Counter ops carry Rust's `value_type` tag. As in Rust, whose counter is an f64, every counter op is written as `"f64"`, and an `"i64"` value on import is read as an f64. Any finite value round-trips, including the i64 endpoints.
- Binary values are written as plain number arrays, including the marker bytes of mergeable containers.

The JSON schema has no binary type, so these arrays come back from `importJsonUpdates` as lists of numbers, in Rust as well. A byte array or a mergeable child therefore does not survive a JSON round trip as binary. Before, `loro.js` passed a `Uint8Array` through, but Rust rejected such JSON.
