# Loro 

Loro is a high‑performance CRDT framework for local‑first apps that keeps state consistent across devices and users, works offline and in real time, automatically merges conflicts, and enables undo/redo and time travel.

[Loro](https://loro.dev) is a high-performance CRDTs library offering Rust, JavaScript and Swift APIs. 

## Common Tasks & Examples

### Getting Started

- Create a document: [`LoroDoc::new()`](struct.LoroDoc.html#method.new) - Initialize a new collaborative document
- Add containers: [`get_text`](struct.LoroDoc.html#method.get_text), [`get_list`](struct.LoroDoc.html#method.get_list), [`get_map`](struct.LoroDoc.html#method.get_map), [`get_tree`](struct.LoroDoc.html#method.get_tree), [`get_movable_list`](struct.LoroDoc.html#method.get_movable_list), [`get_counter` (feature "counter")](struct.LoroDoc.html#method.get_counter)
- Listen to changes: [`subscribe`](struct.LoroDoc.html#method.subscribe) or [`subscribe_root`](struct.LoroDoc.html#method.subscribe_root) - React to document/container modifications
- Export/Import state: [`export`](struct.LoroDoc.html#method.export) and [`import`](struct.LoroDoc.html#method.import) - Save and load documents

### Real-time Collaboration

- Sync between peers: [`export(ExportMode::updates(&vv))`](struct.LoroDoc.html#method.export) + [`import`](struct.LoroDoc.html#method.import) - Exchange incremental updates (see [`ExportMode::updates`](enum.ExportMode.html#method.updates))
- Stream updates: [`subscribe_local_update`](struct.LoroDoc.html#method.subscribe_local_update) - Send changes over WebSocket/WebRTC
- Set unique peer ID: [`set_peer_id`](struct.LoroDoc.html#method.set_peer_id) - Ensure each client has a unique identifier
- Handle conflicts: Automatic - All Loro data types are CRDTs that merge concurrent edits

### Rich Text Editing

- Create rich text: [`get_text`](struct.LoroDoc.html#method.get_text) - Initialize a collaborative text container
- Edit text: [`LoroText::insert`](struct.LoroText.html#method.insert), [`LoroText::delete`](struct.LoroText.html#method.delete), [`LoroText::apply_delta`](struct.LoroText.html#method.apply_delta)
- Apply formatting: [`LoroText::mark`](struct.LoroText.html#method.mark) - Add bold, italic, links, custom styles
- Track cursor positions: [`LoroText::get_cursor`](struct.LoroText.html#method.get_cursor) + [`LoroDoc::get_cursor_pos`](struct.LoroDoc.html#method.get_cursor_pos) - Stable positions across edits
- Configure styles: [`config_text_style`](struct.LoroDoc.html#method.config_text_style) / [`config_default_text_style`](struct.LoroDoc.html#method.config_default_text_style) - Define expand behavior for marks

### Data Structures

- Ordered lists: [`get_list`](struct.LoroDoc.html#method.get_list) - Arrays with [`push`](struct.LoroList.html#method.push), [`insert`](struct.LoroList.html#method.insert), [`delete`](struct.LoroList.html#method.delete)
- Key-value maps: [`get_map`](struct.LoroDoc.html#method.get_map) - Objects with [`insert`](struct.LoroMap.html#method.insert), [`get`](struct.LoroMap.html#method.get), [`delete`](struct.LoroMap.html#method.delete)
- Hierarchical trees: [`get_tree`](struct.LoroDoc.html#method.get_tree) - Trees with [`create`](struct.LoroTree.html#method.create), [`mov`](struct.LoroTree.html#method.mov), [`mov_to`](struct.LoroTree.html#method.mov_to)
- Reorderable lists: [`get_movable_list`](struct.LoroDoc.html#method.get_movable_list) - Drag-and-drop with [`mov`](struct.LoroMovableList.html#method.mov), [`set`](struct.LoroMovableList.html#method.set)
- Counters: [`get_counter` (feature "counter")](struct.LoroDoc.html#method.get_counter) - Distributed counters with [`increment`](struct.LoroCounter.html#method.increment)

### Ephemeral State & Presence

- Not currently provided in the Rust crate. Model presence in your app layer alongside CRDT updates (e.g., via your network transport). Cursors can be shared using [`get_cursor`](struct.LoroText.html#method.get_cursor) data if needed.

### Version Control & History

- Undo/redo: [`UndoManager`](struct.UndoManager.html) - Local undo of user’s own edits
- Time travel: [`checkout`](struct.LoroDoc.html#method.checkout) to any [`Frontiers`](struct.Frontiers.html) - Debug or review history
- Version tracking: [`oplog_vv`](struct.LoroDoc.html#method.oplog_vv), [`state_frontiers`](struct.LoroDoc.html#method.state_frontiers), [`VersionVector`](struct.VersionVector.html)
- Fork documents: [`fork`](struct.LoroDoc.html#method.fork) or [`fork_at`](struct.LoroDoc.html#method.fork_at) - Create branches for experimentation
- Merge branches: [`import`](struct.LoroDoc.html#method.import) - Combine changes from forked documents

### Performance & Storage

- Incremental updates: [`export(ExportMode::updates(&their_vv))`](struct.LoroDoc.html#method.export) - Send only changes (see [`ExportMode::updates`](enum.ExportMode.html#method.updates))
- Compact history: [`export(ExportMode::Snapshot)`](struct.LoroDoc.html#method.export) - Full state with compressed history (see [`ExportMode::Snapshot`](enum.ExportMode.html#variant.Snapshot))
- Shallow snapshots: [`export(ExportMode::shallow_snapshot(&frontiers))`](struct.LoroDoc.html#method.export) - State without partial history (see [`ExportMode::shallow_snapshot`](enum.ExportMode.html#method.shallow_snapshot))

## Documentation

- Start with the [Rust API docs for `LoroDoc`](struct.LoroDoc.html) (container management, versioning, import/export, events)
  That page hosts examples and details for most important methods you’ll use day-to-day.
- [Loro Website](https://loro.dev) for more details and guides
- [Loro Examples](https://github.com/loro-dev/loro-examples) for more examples and guides

## Getting Started

Add to your `Cargo.toml`:

```toml
[dependencies]
loro = "^1"
```

### LoroDoc quick tour

- Containers: [`get_text`](struct.LoroDoc.html#method.get_text), [`get_map`](struct.LoroDoc.html#method.get_map), [`get_list`](struct.LoroDoc.html#method.get_list), [`get_movable_list`](struct.LoroDoc.html#method.get_movable_list), [`get_tree`](struct.LoroDoc.html#method.get_tree)
- Import/Export: [`export(ExportMode::…)`](struct.LoroDoc.html#method.export), [`import`](struct.LoroDoc.html#method.import), [`from_snapshot`](struct.LoroDoc.html#method.from_snapshot)
- Versioning: [`oplog_vv`](struct.LoroDoc.html#method.oplog_vv), [`state_frontiers`](struct.LoroDoc.html#method.state_frontiers), [`checkout`/`checkout_to_latest`](struct.LoroDoc.html#method.checkout), [`revert_to`](struct.LoroDoc.html#method.revert_to), [`fork`](struct.LoroDoc.html#method.fork)
- Events: [`subscribe`](struct.LoroDoc.html#method.subscribe), [`subscribe_root`](struct.LoroDoc.html#method.subscribe_root), [`subscribe_local_update`](struct.LoroDoc.html#method.subscribe_local_update) (send deltas to peers)
- Paths/JSON: [`get_path_to_container`](struct.LoroDoc.html#method.get_path_to_container), [`get_deep_value`](struct.LoroDoc.html#method.get_deep_value) / [`ToJson`](trait.ToJson.html) (`to_json_value()`), optional [`jsonpath` (feature)](struct.LoroDoc.html#method.jsonpath)

Optional cargo features:

```toml
[dependencies]
loro = { version = "^1", features = ["jsonpath"] }
```

## Quick Examples

1) Local edits, change events, and two-peer sync

```rust
use loro::{LoroDoc, ExportMode};
use std::sync::Arc;

let a = LoroDoc::new();
let b = LoroDoc::new();

// Listen for container diffs on `a`
let _changes = a.subscribe_root(Arc::new(|e| {
    println!("changed containers: {}", e.events.len());
}));

a.get_text("text").insert(0, "Hello, Loro!").unwrap();
a.commit(); // events fire on commit/export/import/checkout

// Sync via export/import (send `updates` via your transport)
let updates = a.export(ExportMode::all_updates()).unwrap();
b.import(&updates).unwrap();

assert_eq!(a.get_deep_value(), b.get_deep_value());
```

2) Time travel and revert

```rust
use loro::LoroDoc;

let doc = LoroDoc::new();
let text = doc.get_text("text");
text.insert(0, "Hello").unwrap();
let v0 = doc.state_frontiers();

text.insert(5, ", world").unwrap();
assert_eq!(text.to_string(), "Hello, world");

// Time travel to v0 (read-only)
doc.checkout(&v0).unwrap();
assert_eq!(text.to_string(), "Hello");

// Return to latest and revert
doc.checkout_to_latest();
doc.revert_to(&v0).unwrap();
assert_eq!(text.to_string(), "Hello");
```
