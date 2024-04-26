---
"loro-wasm": minor
"loro-crdt": minor
---

Movable List (#293)

Loro's List supports insert and delete operations but lacks built-in methods for `set` and `move`. To simulate set and move, developers might combine delete and insert. However, this approach can lead to issues during concurrent operations on the same element, often resulting in duplicate entries upon merging.

For instance, consider a list [0, 1, 2]. If user A moves the element '0' to position 1, while user B moves it to position 2, the ideal merged outcome should be either [1, 0, 2] or [1, 2, 0]. However, using the delete-insert method to simulate a move results in [1, 0, 2, 0], as both users delete '0' from its original position and insert it independently at new positions.

To address this, we introduce a MovableList container. This new container type directly supports move and set operations, aligning more closely with user expectations and preventing the issues associated with simulated moves.

## Example

```ts
import { Loro } from "loro-crdt";
import { expect } from "vitest";

const doc = new Loro();
const list = doc.getMovableList("list");
list.push("a");
list.push("b");
list.push("c");
expect(list.toArray()).toEqual(["a", "b", "c"]);
list.set(2, "d");
list.move(0, 1);
const doc2 = new Loro();
const list2 = doc2.getMovableList("list");
expect(list2.length).toBe(0);
doc2.import(doc.exportFrom());
expect(list2.length).toBe(3);
expect(list2.get(0)).toBe("b");
expect(list2.get(1)).toBe("a");
expect(list2.get(2)).toBe("d");
```
