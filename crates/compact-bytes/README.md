# compact-bytes

It's a append-only bytes arena. Appending new bytes will get a pointer to a
slice of the append-only bytes. It will try to reuse the allocated old bytes to
reduce memory usage, if possible.

# Example

```rust
use compact_bytes::CompactBytes;

let mut arena = CompactBytes::new();
let bytes1 = arena.alloc(b"hello");
let bytes2 = arena.alloc(b"world");
assert_eq!(bytes1.as_bytes(), b"hello");
assert_eq!(bytes2.as_bytes(), b"world");

// bytes3 will be a pointer to the same bytes as bytes1
let bytes3 = arena.alloc(b"hello");
assert_eq!(bytes3.as_bytes(), b"hello");
assert_eq!(bytes3.start(), bytes1.start());
assert_eq!(bytes3.start(), 0);
assert_eq!(bytes3.end(), 5);

// Allocatting short bytes will not reuse the old bytes.
// Because it will make merging neighboring slices easier so that when
// serializing the bytes it will be more compact.
let mut bytes4 = arena.alloc(b"h");
assert_eq!(bytes4.start(), 10);
let bytes5 = arena.alloc(b"e");
assert_eq!(bytes5.start(), 11);
// bytes4 and bytes5 can be merged
assert_eq!(bytes4.can_merge(&bytes5), true);
assert!(bytes4.try_merge(&bytes5).is_ok());
```

In advance mode, it will try to reuse the old bytes as much as possible.
So it will break the bytes into small pieces to reuse them.

```rust
use compact_bytes::CompactBytes;
use std::ops::Range;

let mut arena = CompactBytes::new();
let bytes1 = arena.alloc(b"hello");
// it breaks the bytes into 3 pieces "hi ", "hello", " world"
let bytes2: Vec<Range<usize>> = arena.alloc_advance(b"hi hello world");
```

Or you can use `append` to not reuse the old bytes at all.

```rust
use compact_bytes::CompactBytes;

let mut arena = CompactBytes::new();
let bytes1 = arena.alloc(b"hello");
let bytes2 = arena.append(b"hello");
assert_ne!(bytes1.start(), bytes2.start());
```
