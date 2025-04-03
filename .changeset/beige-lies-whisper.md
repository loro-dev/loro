---
"loro-crdt": minor
---

# New Hooks: `pre-commit` and `first-commit-from-peer`

## `doc.subscribePreCommit(listener)`

The `pre-commit` hook enables users to modify commit options before any commit is processed.

This hook is particularly useful because `doc.commit()` is often invoked implicitly in various methods such as `doc.import`, `doc.export`, `doc.checkout`, and `doc.exportJsonUpdates`. Without this hook, users attempting to add custom messages to each commit might miss these implicit commit triggers.

```ts
const doc = new LoroDoc();
doc.setPeerId(0);
doc.subscribePreCommit((e) => {
  e.modifier.setMessage("test").setTimestamp(Date.now());
});
doc.getList("list").insert(0, 100);
doc.commit();
expect(doc.getChangeAt({ peer: "0", counter: 0 }).message).toBe("test");
```

### Advanced Example: Creating a Merkle DAG

By combining `doc.subscribePreCommit` with `doc.exportJsonInIdSpan`, you can implement advanced features like representing Loro's editing history as a Merkle DAG:

```ts
const doc = new LoroDoc();
doc.setPeerId(0);
doc.subscribePreCommit((e) => {
  const changes = doc.exportJsonInIdSpan(e.changeMeta)
  expect(changes).toHaveLength(1);
  const hash = crypto.createHash('sha256');
  const change = {
    ...changes[0],
    deps: changes[0].deps.map(d => {
      const depChange = doc.getChangeAt(idStrToId(d))
      return depChange.message;
    })
  }
  hash.update(JSON.stringify(change));
  const sha256Hash = hash.digest('hex');
  e.modifier.setMessage(sha256Hash);
});

console.log(change); // The output is shown below
doc.getList("list").insert(0, 100);
doc.commit();
// Change 0
// {
//   id: '0@0',
//   timestamp: 0,
//   deps: [],
//   lamport: 0,
//   msg: undefined,
//   ops: [
//     {
//       container: 'cid:root-list:List',
//       content: { type: 'insert', pos: 0, value: [100] },
//       counter: 0
//     }
//   ]
// }


doc.getList("list").insert(0, 200);
doc.commit();
// Change 1
// {
//   id: '1@0',
//   timestamp: 0,
//   deps: [
//     '2af99cf93869173984bcf6b1ce5412610b0413d027a5511a8f720a02a4432853'
//   ],
//   lamport: 1,
//   msg: undefined,
//   ops: [
//     {
//       container: 'cid:root-list:List',
//       content: { type: 'insert', pos: 0, value: [200] },
//       counter: 1
//     }
//   ]
// }

expect(doc.getChangeAt({ peer: "0", counter: 0 }).message).toBe("2af99cf93869173984bcf6b1ce5412610b0413d027a5511a8f720a02a4432853");
expect(doc.getChangeAt({ peer: "0", counter: 1 }).message).toBe("aedbb442c554ecf59090e0e8339df1d8febf647f25cc37c67be0c6e27071d37f");
```

## `doc.subscribeFirstCommitFromPeer(listener)`

The `first-commit-from-peer` event triggers when a peer performs operations on the document for the first time.
This provides an ideal point to associate peer information (such as author identity) with the document.

```ts
const doc = new LoroDoc();
doc.setPeerId(0);
doc.subscribeFirstCommitFromPeer((e) => {
  doc.getMap("users").set(e.peer, "user-" + e.peer);
});
doc.getList("list").insert(0, 100);
doc.commit();
expect(doc.getMap("users").get("0")).toBe("user-0");
```
