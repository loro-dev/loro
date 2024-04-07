# Changelog

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
