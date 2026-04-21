<div align="center">
  <h1><code>append-only-bytes</code></h2>
  <h3><a href="https://docs.rs/append-only-bytes">Documentation</a></h3>
  <p></p>
</div>

If an array is append-only and guarantees that all the existing data is immutable, we can safely share slices of this array across threads, while the owner can still safely append new data to it.

This is safe because no mutable byte has more than one owner. If there isn't enough capacity for a new append, `AppendOnlyBytes` will not deallocate the old memory if a `ByteSlice` is referring to it. The old memory will be deallocated only when all the `ByteSlice`s referring to it are dropped.

# Example

```rust
use append_only_bytes::{AppendOnlyBytes, BytesSlice};
let mut bytes = AppendOnlyBytes::new();
bytes.push_slice(&[1, 2, 3]);
let slice: BytesSlice = bytes.slice(1..);
bytes.push_slice(&[4, 5, 6]);
assert_eq!(&*slice, &[2, 3]);
assert_eq!(bytes.as_bytes(), &[1, 2, 3, 4, 5, 6])
```

# Features

- `serde`: support serde serialization and deserialization
- `u32_range`: support `u32` range for `ByteSlices` method
