---
"loro-crdt": patch
---

Correct LCA unmatched-branch detection when an explicit dependency already
covers the same peer's implicit predecessor. Keep those causally newer imports
on the current replay base, and avoid rebuilding unchanged list-like containers
when a genuinely concurrent update needs a conservative base containing a large
common history.
