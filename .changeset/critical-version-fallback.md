---
"loro-crdt": patch
---

Retreat the conservative replay base to the latest single-head critical
version of the two versions' combined history instead of the beginning of
history. When an import or checkout involves a genuinely concurrent branch,
the causal replay now starts at the most recent point that no concurrency
crosses (in the sense of Eg-walker's critical versions), skipping the
fully-synced common prefix that the old empty-version fallback replayed.
