---
"loro-crdt": patch
---

Speed up local commits on a document that has exported or imported a snapshot and has a `subscribeLocalUpdates` listener. Each commit looked its own change up in the encoded history store although the change was still in memory; once the history outgrew the default 4 MiB block cache (for example one text insert of 5 MB) that lookup decompressed a history block on every commit (about 6 ms per keystroke at 5 MB and 22 ms at 20 MB, now 0.005 ms).
