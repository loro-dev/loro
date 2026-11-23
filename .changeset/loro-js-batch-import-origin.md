---
"loro.js": patch
---

Match Rust's multi-blob `importBatch` event labels: keep `by: "import"` and emit `origin: "checkout"`. Single-blob batches continue to use ordinary import event labels.
