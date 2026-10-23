---
{}
---

Fix Rust `loro-delta` text insertion dropping the final chunk of strings longer than 128 bytes. Share UTF-8-aware chunking between append and middle insertion, preventing capacity panics when a multibyte character crosses a chunk boundary.
