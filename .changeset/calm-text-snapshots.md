---
"loro-js": patch
---

Reduce large Text snapshot size, import time, and retained memory by coalescing
state spans, hydrating bounded chunks, and using dense ID storage when safe.
