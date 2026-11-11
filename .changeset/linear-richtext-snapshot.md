---
{}
---

Validate richtext snapshot text lengths with one Unicode scalar scan instead of rescanning the remaining text for every span. This avoids quadratic validation work when decoding fragmented shallow snapshots.
