# Changelog

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
