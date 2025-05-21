# Changelog

## 1.5.7

### Patch Changes

- 70a3bf3: Feat: support `group_start` and `group_end` for UndoManager #720

## 1.5.6

### Patch Changes

- 60876bb: Feat: redact

## 1.5.5

### Patch Changes

- 0bfbb3b: fix: update EphemeralStore to support generic types (#718)

## 1.5.4

### Patch Changes

- 37c2a17: fix: checkout should renew txn if not detached

## 1.5.3

### Patch Changes

- ed4fe83: fix: from_snapshot with shallow snapshot err #712

## 1.5.2

### Patch Changes

- 81c7bb7: fix ephemeral store recursive use by adding mutex in the inner
- bf94a03: feat: add functionality to delete and hide empty root containers #708

## 1.5.1

### Patch Changes

- 742cf7d: Fix memory leak caused by wasm-bindgen

  - https://github.com/rustwasm/wasm-bindgen/issues/3854

## 1.5.0

### Minor Changes

- 8dfcad4: # New Hooks: `pre-commit` and `first-commit-from-peer`

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
    const changes = doc.exportJsonInIdSpan(e.changeMeta);
    expect(changes).toHaveLength(1);
    const hash = crypto.createHash("sha256");
    const change = {
      ...changes[0],
      deps: changes[0].deps.map((d) => {
        const depChange = doc.getChangeAt(idStrToId(d));
        return depChange.message;
      }),
    };
    hash.update(JSON.stringify(change));
    const sha256Hash = hash.digest("hex");
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

  expect(doc.getChangeAt({ peer: "0", counter: 0 }).message).toBe(
    "2af99cf93869173984bcf6b1ce5412610b0413d027a5511a8f720a02a4432853",
  );
  expect(doc.getChangeAt({ peer: "0", counter: 1 }).message).toBe(
    "aedbb442c554ecf59090e0e8339df1d8febf647f25cc37c67be0c6e27071d37f",
  );
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

- a997885: # `EphemeralStore`: An Alternative to Awareness

  Awareness is commonly used as a state-based CRDT for handling ephemeral states in real-time collaboration scenarios, such as cursor positions and application component highlights. As application complexity grows, Awareness may be set in multiple places, from cursor positions to user presence. However, the current version of Awareness doesn't support partial state updates, which means even minor mouse movements require synchronizing the entire Awareness state.

  ```ts
  awareness.setLocalState({
    ...awareness.getLocalState(),
    x: 167,
  });
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

### Patch Changes

- 742842f: fix: apply multiple styles via text delta at the end "\n" char #692
- 4cb7ae3: feat: get ops from current txn as json #676

## 1.4.6

### Patch Changes

- 0b0ac7c: fix: entity index when the tree is empty

## 1.4.5

### Patch Changes

- aab07c6: feat: set default config for text style #669
- 6cf06e5: fix: detached loro text issues #665

## 1.4.4

### Patch Changes

- 28d1264: feat(wasm): enhance toJsonWithReplacer to handle nested containers in replacer returned value
- 28d1264: fix(wasm): add toJSON to LoroText

  Now all containers have toJSON method.

## 1.4.3

### Patch Changes

- 2a82396: feat: add new ways to control commit options (#656)
- 2a82396: fix: mark err on detached LoroText (#659)

## 1.4.2

### Patch Changes

- e0948d8: feat: add container existence check methods & avoid panic in wasm/js #651
- 9955500: fix: an internal iter_change err that may cause fork_at panic #649

## 1.4.1

### Patch Changes

- 9090c4d: fix: memory leak issue

## 1.4.0

### Minor Changes

- 2f3364a: refactor!: use better data type for doc.diff #646

### Patch Changes

- 72699ae: fix: getting values by path in LoroTree (#643)

## 1.3.5

### Patch Changes

- c2a61f3: fix: improve shallow snapshot and JSON export handling #639

## 1.3.4

### Patch Changes

- b58e6bd: fix: should be able to call subscription after diffing #637

## 1.3.3

### Patch Changes

- 8fdb25e: fix: move tree node within the self parent with 16 siblings #635

## 1.3.2

### Patch Changes

- a168063: refactor: hold doc reference in handler (#624)
- a168063: fix: a few LoroCounter errors (#626)

## 1.3.1

### Patch Changes

- 07500da: fix: map.keys() may return keys from deleted entries #618

## 1.3.0

### Minor Changes

- ddafb7e: feat: diff, applyDiff, and revertTo #610

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
  expect(diff).toMatchSnapshot();
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

- ac51ceb: feat: add exportJsonInIdSpan and make peer compression optional
- 8039e44: feat: find id spans between #607

### Patch Changes

- 9c1005d: fix: should not merge remote changes due to small interval

## 1.2.7

### Patch Changes

- da24910: fix: should commit before travel_change_ancestors #599

## 1.2.6

### Patch Changes

- d552955: Make getByPath work for "tree/0/key"
- df81aec: Better event ordering

## 1.2.5

### Patch Changes

- 9faa149: Fix detach + attach error

## 1.2.4

### Patch Changes

- 5aa7985: Fixed LoroTree's incorrect index when moving a node within its current parent

## 1.2.3

### Patch Changes

- 42949c0: Fix VersionVector ownership issue in WASM binding
- 1ca1275: feat: UndoManager's onPush now can access the change event

## 1.2.2

### Patch Changes

- 3b7a738: Add getShallowValue and toJsonWIthReplacer

  - Add getShallowValue for each container (#581)
  - Implement toJsonWithReplacer method for LoroDoc to customize JSON serialization (#582)
  - Rename importUpdateBatch into importBatch & refine type (#580)

## 1.2.1

### Patch Changes

- adb6ab8: fix: panic when returned non-boolean value from text.iter(f) #578

## 1.2.0

### Minor Changes

- 01fccc5: Return ImportStatus in the import_batch method

### Patch Changes

- d08a865: fix: getOrCreateContainer should not throw if value is null #576

## 1.1.4

### Patch Changes

- 0325061: Fix a deadloop case when importing updates (#570)

## 1.1.3

### Patch Changes

- d6966ac: The fractional index in LoroTree is now enabled by default with jitter=0.

  To reduce the cost of LoroTree, if the `index` property in LoroTree is unused, users can still
  call `tree.disableFractionalIndex()`. However, in the new version, after disabling the fractional
  index, `tree.moveTo()`, `tree.moveBefore()`, `tree.moveAfter()`, and `tree.createAt()` will
  throw an error

## 1.1.2

### Patch Changes

- 70c4942: Add base64 build target
- 35e7ea5: Add changeCount and opCount methods

## 1.1.1

### Patch Changes

- 9abeb81: Add methods to modify VV
- ee26952: Add isDeleted() method to each container

## 1.1.0

### Minor Changes

- 6e878d2: Feat add API to query creators, the last editors/movers
- 778ca54: Feat: allow users to query the changed containers in the target id range

### Patch Changes

- 6616101: Perf: optimize importBatch

  When using importBatch to import a series of snapshots and updates, we should import the snapshot with the greatest version first.

- 6e878d2: Feat: getLastEditor on LoroMap
- 8486234: Fix get encoded blob meta

## 1.0.9

### Patch Changes

- 7bf6db7: Add `push` to LoroText and `pushContainer` to LoroList LoroMovableList
- 9b60d01: Define the behavior of `doc.fork()` when the doc is detached

  It will fork at the current state_frontiers, which is equivalent to calling `doc.fork_at(&doc.state_frontiers())`

## 1.0.8

### Patch Changes

- 62a3a93: Merge two js packages

## 1.0.8-alpha.3

### Patch Changes

- Fix build for bundler

## 1.0.8-alpha.2

### Patch Changes

- Fix build script for web target

## 1.0.8-alpha.1

### Patch Changes

- Include the build for web

## 1.0.8-alpha.0

### Patch Changes

- Refactor simplify js binding

## 1.0.7

### Patch Changes

- Skip published version

## 1.0.1

### Patch Changes

- Release v1.0.1

## 1.0.0

### Patch Changes

- dd3bd92: Release v1.0

## 1.0.0-beta.5

### Patch Changes

- - Fork at should restore detached state (#523)
  - Subscription convert error (#525)

## 1.0.0-beta.4

### Patch Changes

- Fix: ForkAt should inherit the config and auto commit from the original doc

## 1.0.0-beta.3

### Patch Changes

- - Wasm api 1.0 (#521)
  - Rename wasm export from (#519)
  - Rename tree event (#520)

## 1.0.0-beta.2

### Patch Changes

- _(wasm)_ Add methods to encode and decode Frontiers (#517)
- Avoid auto unsubscribe (due to gc) in js env (#516)

## 1.0.0-beta.1

### Patch Changes

- Performance improvement and bug fixes

  ### 🚀 Features

  - Redact (#504)

  ### 🐛 Bug Fixes

  - Ffi Subscription (#505)
  - Ffi remove try unwrap (#506)
  - Movable list undo impl (#509)
  - Tracker skip applied deletion error (#512)
  - IsContainerDeleted cache err (#513)

  ### 📚 Documentation

  - Refine wasm docs

  ### ⚡ Performance

  - Optimize shrink frontiers
  - Optimize batch container registrations on arena (#510)
  - Optimize high concurrency performance (#514)
  - Use better data structure for frontiers (#515)

  ### Perf

  - Commit speed & text insert cache (#511)

## 1.0.0-alpha.5

### Patch Changes

- ## Fix

  - Use release build

  ## Test

  - Add compatibility tests (#503)

## 1.0.0-alpha.4

### Patch Changes

- ### 🚀 Features

  - _(wasm)_ Commit message & get pending ops length (#477)
  - Update text by line (#480)
  - Add clear methods (#478)
  - Travel change's ancestors (#483)
  - Compact state store
  - Add FFI for Loro (#420)
  - Add dag allocation tree algorithm (#415)
  - Add import status (#494)

  ### 🐛 Bug Fixes

  - Get correct tree_cache current vv when retreating (#476)
  - Gc snapshot error (#481)
  - Checkout into middle of marks
  - Checkout diff-calc cache issue
  - Return err if snapshot container has unknown container (#488)
  - Do not set peer id with max (#491)
  - Fork error (#493)
  - FFI new sub import status (#497)
  - Create event cannot find parent (#498)

  ### 🚜 Refactor

  - [**breaking**] Don't wait for `commit` to update version info
  - Avoid footgun of impl ord for cid
  - Loro import function should return LoroEncodeError (#487)
  - [**breaking**] Better event api (#489)
  - Change the first param of travel change from id to ids (#492)
  - [**breaking**] List state snapshot schema for v1.0 (#485)

  ### ⚡ Performance

  - Make shrink frontiers faster when the peer num is large (#482)
  - Optimize tree cache find children speed
  - Avoid memory leak when forking repeatedly (#500)
  - Optimize kv export_all by reusing encoded block (#501)
  - Optimize speed of large maps (#496)
  - Optimize diff calc cache use (#475)

  ### 🧪 Testing

  - Make awareness more robust
  - Bench large folder with 1M files & 100M ops (#495)

  ### ⚙️ Miscellaneous Tasks

  - Use cached diff calc

## 1.0.0-alpha.3

### Patch Changes

- ### 🐛 Bug Fixes

  - Cursor behavior when using gc-snapshot (#472)
  - _(wasm)_ Type err

  ### ⚙️ Miscellaneous Tasks

  - Make tree parent id pub on loro crate

  ### Feat

  - Allow editing on detached mode (#473)

  ### Fix

  - Get tree's alive children correctly (#474)
  - Should not emit event when exporting gc-snapshot (#471)

## 1.0.0-alpha.2

### Patch Changes

- ### 🚀 Features

  - Fork doc at the target version (#469)

  ### 🚜 Refactor

  - BREAKING CHANGE: Use hierarchy value for tree value (#470)

## 1.0.0-alpha.1

### Patch Changes

- ### 🚀 Features

  - Get shallow value of doc (#463)
  - Add state only snapshot & refine check slow test
  - Add new cid method to js binding
  - Jsonpath experimental support (#466)

  ### 🐛 Bug Fixes

  - Raise error if perform action on a deleted container (#465)
  - Raise error if moving a deleted node
  - Export snapshot error on a gc doc

  ### 🚜 Refactor

  - Tree contains & isDeleted (#467)

  ### 🧪 Testing

  - Check state correctness on shallow doc

## 1.0.0-alpha.0

- Better encode schema that can be 100x faster
- Less memory usage
- You can trim needless history in snapshot now
- Better architecture and extensibility

## 0.16.12

### Patch Changes

- 46e21fc: Fix tree move issues

## 0.16.11

### Patch Changes

- dce00ab: Make loro-wasm work in cloudflare worker

## 0.16.10

### Patch Changes

- 7cf54e8: Fix batch importing with snapshot

## 0.16.9

### Patch Changes

- a761430: Fix build script

## 0.16.8

### Patch Changes

- 38b4bcf: Add text update API

  - Remove the patch for crypto
  - Add text update API (#404)
  - Check invalid root container name (#411)

  ### 🐛 Bug Fixes

  - Workaround lldb bug make loro crate debuggable (#414)
  - Delete the **bring back** tree node from the undo container remap (#423)

  ### 📚 Documentation

  - Fix typo
  - Refine docs about event (#417)

  ### 🎨 Styling

  - Use clippy to perf code (#407)

  ### ⚙️ Miscellaneous Tasks

  - Add test tools (#410)

## 0.16.7

### Patch Changes

- 45c98d5: Better text APIs and bug fixes

  ### 🚀 Features

  - Add insert_utf8 and delete_utf8 for Rust Text API (#396)
  - Add text iter (#400)
  - Add more text api (#398)

  ### 🐛 Bug Fixes

  - Tree undo when processing deleted node (#399)
  - Tree diff calc children should be sorted by idlp (#401)
  - When computing the len of the map, do not count elements that are None (#402)

  ### 📚 Documentation

  - Update wasm docs
  - Rm experimental warning

  ### ⚙️ Miscellaneous Tasks

  - Update fuzz config
  - Pnpm
  - Rename position to fractional_index (#381)

## 0.16.6

### Patch Changes

- 1e94248: Add `.fork()` to duplicate the doc

## 0.16.5

### Patch Changes

- 439e4e9: Update pkg desc

## 0.16.4

### Patch Changes

- afac347: feat: implement `Counter` and expose it to js side

## 0.16.4-alpha.0

### Patch Changes

- Export/import JSON schema

## 0.16.3

### Patch Changes

- 6d47015: Make cursors transformation better in undo/redo loop
- dc55055: Perf(wasm) cache text.toDelta

## 0.16.2

### Patch Changes

- 34f6064: Better undo events & transform cursors by undo manager (#369)

  #### 🧪 Testing

  - Enable compatibility test (#367)

## 0.16.1

### Patch Changes

- 5cd80b0: Refine undo impl

  - Add "undo" origin for undo and redo event
  - Allow users to skip certain local operations
  - Skip undo/redo ops that are not visible to users
  - Add returned bool value to indicate whether undo/redo is executed

## 0.16.0

### Minor Changes

- c12c2b9: Movable Tree Children & Undo

  #### 🐛 Bug Fixes

  - Refine error message on corrupted data (#356)
  - Add MovableList to CONTAINER_TYPES (#359)
  - Better jitter for fractional index (#360)

  #### 🧪 Testing

  - Add compatibility tests (#357)

  #### Feat

  - Make the encoding format forward and backward compatible (#329)
  - Undo (#361)
  - Use fractional index to order the children of the tree (#298)

  #### 🐛 Bug Fixes

  - Tree fuzz sort value (#351)
  - Upgrade wasm-bindgen to fix str free err (#353)

## 0.15.3

### Patch Changes

- 43506cc: Fix unsound issue caused by wasm-bindgen

  #### 🐛 Bug Fixes

  - Fix potential movable list bug (#354)
  - Tree fuzz sort value (#351)
  - Upgrade wasm-bindgen to fix str free err (#353)

  #### 📚 Documentation

  - Simplify readme (#352)

## 0.15.2

### Patch Changes

- e30678d: Perf: fix deletions merge

  #### 🐛 Bug Fixes

  - _(wasm)_ Movable list .kind() (#342)

  #### ⚡ Performance

  - Delete span merge err (#348)

  #### ⚙️ Miscellaneous Tasks

  - Warn missing debug impl (#347)

  <!-- generated by git-cliff -->

## 0.15.1

### Patch Changes

- 04c6290: Bug fixes and improvements.

  #### 🐛 Bug Fixes

  - Impl a few unimplemented! for movable tree (#335)
  - Refine ts type; reject invalid operations (#334)
  - Get cursor err on text and movable list (#337)
  - Missing MovableList in all container type (#343)
  - Upgrade generic-btree to allow large btree (#344)

  #### 📚 Documentation

  - Add warn(missing_docs) to loro and loro-wasm (#339)
  - Minor fix on set_change_merge_interval api (#341)

  #### ⚙️ Miscellaneous Tasks

  - Skip the checking if not debug_assertions (#340)

  <!-- generated by git-cliff -->

## 0.15.0

### Minor Changes

- 35b9b6e: Movable List (#293)

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

## 0.14.6

### Patch Changes

- 24cf9b9: Bug Fix

  #### 🐛 Bug Fixes

  - Attached container can be inserted to `Map` or `List` (#331)

## 0.14.5

### Patch Changes

- 73e3ba5: Bug Fix

  #### 🐛 Bug Fixes

  - _(js)_ Allow convert from undefined to LoroValue (#323)

  #### 🚜 Refactor

  - Refine ts type (#322)

## 0.14.4

### Patch Changes

- 598d97e: ### 🚜 Refactor

  - Refine the TS Type of Awareness
  - Parse Uint8array to LoroValue::Binary (#320)

  ### 📚 Documentation

  - Update how to publish new npm pkgs

## 0.14.3

### Patch Changes

- a1fc2e3: Feat: Awareness (#318)

## 0.14.2

### Patch Changes

- Refactor rename `StablePosition` to `Cursor`

  - Rename stable pos to cursor (#317)

  <!-- generated by git-cliff -->

## 0.14.1

### Patch Changes

- Supports Cursors

  #### 🚀 Features

  - Cursors (#290)

## 0.14.0

### Minor Changes

- Improved API

  ### 🚀 Features

  - Access value/container by path (#308)
  - Decode import blob meta (#307)

  ### 🐛 Bug Fixes

  - Decode iter return result by updating columnar to 0.3.4 (#309)

  ### 🚜 Refactor

  - Replace "local" and "fromCheckout" in event with "triggeredBy" (#312)
  - Add concrete type for each different container (#313)
  - _(ts)_ Make types better (#315)

  ### 📚 Documentation

  - Refine wasm docs (#304)
  - Clarify that peer id should be convertible to a u64 (#306)

  ### ⚙️ Miscellaneous Tasks

  - Add coverage report cli (#311)

## 0.13.1

### Patch Changes

- Fix type errors and conversion from js->rust error

## 0.13.0

### Minor Changes

- BREAKING CHANGE: `detached` mode for Containers #300

  Now creating sub-containers is much easier.

  A container can be either attached to a document or detached. When it's detached, its history/state is not persisted. You can attach a container to a document by inserting it into an existing attached container. Once a container is attached, its state, along with all of its descendants's states, will be recreated in the document. After attaching, the container and its descendants will each have their corresponding "attached" version of themselves.

  When a detached container x is attached to a document, you can use `x.getAttached()` to obtain the corresponding attached container.

  When we use const text = new LoroList(), it's not attached to a doc. But we can insert it into a doc by map.insertContainer(”t”, text), where the map is attached. But if we want the operations on the text to be recorded to the doc, we now need to get its attached version. So we can use “let attachedText = text.getAttached()”

## 0.12.0

### Minor Changes

- Add getParent and getOrCreate

## 0.11.1

### Patch Changes

- Fix batch import

## 0.11.0

### Minor Changes

- Fix a few bugs and include BREAKING CHANG refactors

  - fix: should not reset the state when calling checkout to latest (#265)
  - refactor: only send a event for one `import`/`transaction`/`checkout` (#263)
  - perf: optimize snapshot encoding speed (#264)
  - feat: remove deleted set in tree state and optimize api (#259)

## 0.10.1

### Patch Changes

- fix: remove checking after checkout

## 0.10.0

### Minor Changes

- New encoding schema
  - BREAKING CHANGE: refactor: Optimizing Encoding Representation for Child Container Creation to Reduce Document Size (#247)
  - feat: compare frontiers causal order (#257)
  - docs: update docs about rich text style (#258)

## 0.9.4

### Patch Changes

- Fix a few richtext time travel issues

## 0.9.3

### Patch Changes

- feat: add getChangeAtLamport

## 0.9.2

### Patch Changes

- Fix a few rich text issue
  - fix: time travel back should be able to nullify rich text span (#254)
  - fix: formalize apply delta method (#252)
  - fix: how to find best insert pos for richtext & expand type reverse behavior (#250)

## 0.9.1

### Patch Changes

- Fix use consistnt peer id repr and expose VersionVector type

## 0.9.0

### Minor Changes

- Refine the rich text CRDT in Loro

## 0.8.0

### Minor Changes

- Stabilize encoding and fix several issues related to time travel

## 0.7.2-alpha.4

### Patch Changes

- Fix encoding value err

## 0.7.2-alpha.3

### Patch Changes

- Fix export compressed snapshot

## 0.7.2-alpha.2

### Patch Changes

- Add compressed method

## 0.7.2-alpha.1

### Patch Changes

- Fix v0 exports

## 0.7.2-alpha.0

### Patch Changes

- Add experimental encode methods

## 0.7.1

### Patch Changes

- Fix a few richtext errors

## 0.7.0

### Minor Changes

- refactor: remove setPanicHook and call it internally when loaded

## 0.6.5

### Patch Changes

- Fix checkout err on seq data

## 0.6.4

### Patch Changes

- Fix time travel issue #211

## 0.6.1

### Patch Changes

- 6753c2f: Refine loro-crdt api

## 0.6.0

### Minor Changes

- Improve API of event

All notable changes to this project will be documented in this file. See [standard-version](https://github.com/conventional-changelog/standard-version) for commit guidelines.
