---
"loro-js": patch
---

Reduce large Text snapshot size, import/export time, and temporary memory by
coalescing state spans, traversing compact Text storage directly, building
validated snapshot treaps linearly, and validating deferred history without
materializing operation objects.
