---
"loro-crdt": minor
---

# `EphemeralStore`: An Alternative to Awareness

Awareness is commonly used as a state-based CRDT for handling ephemeral states in real-time collaboration scenarios, such as cursor positions and application component highlights. As application complexity grows, Awareness may be set in multiple places, from cursor positions to user presence. However, the current version of Awareness doesn't support partial state updates, which means even minor mouse movements require synchronizing the entire Awareness state.

```ts
awareness.setLocalState(
    {
        ...awareness.getLocalState(),
        x: 167
    }
);
```

Since Awareness is primarily used in real-time collaboration scenarios where consistency requirements are relatively low, we can make it more flexible. We've introduced `EphemeralStore` as an alternative to `Awareness`. Think of it as a simple key-value store that uses timestamp-based last-write-wins for conflict resolution. You can choose the appropriate granularity for your key-value pairs based on your application's needs, and only modified key-value pairs are synchronized.

## Examples

```ts
import {
    EphemeralStore,
    EphemeralListener,
    EphemeralStoreEvent,
} from "loro-crdt";

const store = new EphemeralStore();
// Set ephemeral data
store.set("loro-prosemirror", {
    anchor: ...,
    focus: ...,
    user: "Alice"
});
store.set("online-users", ["Alice", "Bob"]);

expect(storeB.get("online-users")).toEqual(["Alice", "Bob"]);
// Encode only the data for `loro-prosemirror`
const encoded = store.encode("loro-prosemirror")

store.subscribe((e: EphemeralStoreEvent) => {
    // Listen to changes from `local`, `remote`, or `timeout` events
});
```
