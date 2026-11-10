---
"loro-crdt": minor
---

`LoroMovableList` can read the stable id of each element.

The id is assigned when the element is inserted and stays with that element when it is moved or its value is replaced. Two rows with the same value remain distinguishable, including after concurrent moves. The id is still there after checking out an older version and after loading a state-only or shallow snapshot, when the insert that created the element may no longer be in the history. An index past the end, or an id that is not visible at the current version, returns nothing.

- Rust: `LoroMovableList::get_id_at`, `get_index_of_id`, and `get_ids`, using the existing `IdLp` (`L{lamport}@{peer}`).
- JS: `getIdAt`, `getIndexOfId`, and `getIds`. An id is `{ peer, lamport }`.

This is the element's id, not the list-item id a cursor anchors. List diff events are unchanged.
