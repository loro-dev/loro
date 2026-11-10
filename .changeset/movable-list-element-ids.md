---
"loro-crdt": minor
---

`LoroMovableList` can read the stable id of each element.

The id is assigned when the element is inserted and stays with that element when it is moved or its value is replaced. Two rows with the same value remain distinguishable, including after concurrent moves. The id is still there after checking out an older version and after loading a state-only or shallow snapshot, when the insert that created the element may no longer be in the history. An index past the end, or an id that is not visible at the current version, returns nothing.

- Rust: `LoroMovableList::get_elem_id_at`, `get_index_of_elem_id`, and `get_elem_ids`, using the existing `IdLp` (`L{lamport}@{peer}`).
- JS: `getElemIdAt`, `getIndexOfElemId`, and `getElemIds`. An id is `{ peer, lamport }`.

This is the element's id, not the slot id a cursor anchors. `get_cursor` and `LoroList::get_id_at` (JS `LoroList.getIdAt`) return that slot id, `{ peer, counter }`. List diff events are unchanged.
