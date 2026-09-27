---
"loro.js": patch
---

`exportJsonUpdates` in `loro.js` now writes Counter ops with Rust's `value_type` tag (`"f64"`, `"i64"`), and binary values, including mergeable-container markers, as plain number arrays like Rust. Rust previously rejected `loro.js` JSON that contained a counter op or a binary value. `importJsonUpdates` keeps an `i64` counter as `i64`.
