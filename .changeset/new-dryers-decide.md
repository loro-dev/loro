---
"loro-crdt": minor
---

feat: diff, applyDiff, and revertTo #610

Add new version-control-related primitives: 

- **`diff(from, to)`**: calculate the difference between two versions. The returned results have similar structures to the differences in events.
- **`revertTo(targetVersion)`**: revert the document back to the target version. The difference between this and `checkout(targetVersion)` is this method will generate a series of new operations, which will transform the current doc into the same as the target version. 
- **`applyDiff(diff)`**: you can use it to apply the differences generated from `diff(from, to)`. 

You can use these primitives to implement version-control functions like `squash` and `revert`.

# Examples

`revertTo`

```ts
const doc = new LoroDoc();
doc.setPeerId("1");
doc.getText("text").update("Hello");
doc.commit();
doc.revertTo([{ peer: "1", counter: 1 }]);
expect(doc.getText("text").toString()).toBe("He");
```

`diff`

```ts
const doc = new LoroDoc();
doc.setPeerId("1");
// Text edits with formatting
const text = doc.getText("text");
text.update("Hello");
text.mark({ start: 0, end: 5 }, "bold", true);
doc.commit();

// Map edits
const map = doc.getMap("map");
map.set("key1", "value1");
map.set("key2", 42);
doc.commit();

// List edits
const list = doc.getList("list");
list.insert(0, "item1");
list.insert(1, "item2");
list.delete(1, 1);
doc.commit();

// Tree edits
const tree = doc.getTree("tree");
const a = tree.createNode();
a.createNode();
doc.commit();

const diff = doc.diff([], doc.frontiers());
expect(diff).toMatchSnapshot()
```

```js
{
  "cid:root-list:List": {
    "diff": [
      {
        "insert": [
          "item1",
        ],
      },
    ],
    "type": "list",
  },
  "cid:root-map:Map": {
    "type": "map",
    "updated": {
      "key1": "value1",
      "key2": 42,
    },
  },
  "cid:root-text:Text": {
    "diff": [
      {
        "attributes": {
          "bold": true,
        },
        "insert": "Hello",
      },
    ],
    "type": "text",
  },
  "cid:root-tree:Tree": {
    "diff": [
      {
        "action": "create",
        "fractionalIndex": "80",
        "index": 0,
        "parent": undefined,
        "target": "12@1",
      },
      {
        "action": "create",
        "fractionalIndex": "80",
        "index": 0,
        "parent": "12@1",
        "target": "13@1",
      },
    ],
    "type": "tree",
  },
}
```
