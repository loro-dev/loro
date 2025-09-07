---
"loro-crdt": minor
---

Distinguish explicit vs implicit empty commit behavior for commit options.

- Explicit commits (user-invoked `commit()` / `commit_with(...)`): if the transaction is empty, commit options (message/timestamp/origin) are swallowed and will NOT carry over.
- Implicit commits (e.g., `export`, `checkout` internal barriers): if the transaction is empty, message/timestamp/origin are preserved for the next transaction.

Rationale: align behavior with intent. Explicit commits “finalize now”, so empty commits should not leak options. Implicit commits act as processing barriers and should not destroy user-provided options for the next real change.

Note: This refines behavior without changing the API.
