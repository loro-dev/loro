---
"loro-crdt": patch
---

Speed up local commits on a document that has exported or imported a snapshot and has a `subscribeLocalUpdates` listener. Each commit looked its own change up in the encoded history store although the change was still in memory; once the history outgrew the 4 MiB block cache (for example one text insert of 5 MB) that lookup decompressed a history block on every commit (5 to 7 ms per keystroke, now 0.007 ms).
