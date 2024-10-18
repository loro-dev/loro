# Changelog

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
