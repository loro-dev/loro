# Loro CRDT WebAssembly Component Demo

This demo showcases how to use Loro CRDT as a WebAssembly Component using the [WebAssembly Interface Types (WIT)](https://component-model.bytecodealliance.org/design/wit.html) specification.

## Overview

The `loro-wit` crate provides WebAssembly Component Model bindings for the Loro CRDT library. This enables using Loro from any language that supports the WebAssembly Component Model, including:

- JavaScript/TypeScript (via [jco](https://github.com/bytecodealliance/jco))
- Rust (via [wasmtime](https://wasmtime.dev/))
- Python (via [wasmtime-py](https://github.com/bytecodealliance/wasmtime-py))
- Go, C/C++, and more

## WIT Interface

The WIT interface defines the following interfaces:

- **loro-doc**: Main document interface with `Doc` resource
  - Create documents, get containers, commit changes
  - Export/import updates for sync
  - Fork documents

- **loro-text**: Text CRDT operations
  - Insert, delete, get content

- **loro-map**: Map CRDT operations
  - Insert string/number/boolean values
  - Get, delete, check keys

- **loro-list**: List CRDT operations
  - Insert at position, push, get, delete

## Building the Component

First, build the Loro WebAssembly Component:

```bash
# From the repository root
cargo component build --release -p loro-wit
```

This creates `target/wasm32-wasip1/release/loro_wit.wasm`.

## Running the Demos

### JavaScript Demo (jco)

The jco demo transpiles the WebAssembly Component to JavaScript for use in Node.js or browsers.

```bash
cd examples/wit-demo/jco

# Install dependencies
npm install

# Build (transpile the component)
npm run build

# Run the demo
npm run demo
```

### Rust Demo (wasmtime)

The wasmtime demo shows how to host the component from a Rust application.

```bash
cd examples/wit-demo/wasmtime-host

# Run the demo (it will build and execute)
cargo run --release
```

## Example Usage

### JavaScript (after jco transpilation)

```javascript
import { loroDoc, loroText, loroMap } from './gen/loro.js';

// Create a document
const doc = new loroDoc.Doc();

// Work with text
const text = doc.getText('content');
loroText.insert(doc, text, 0, 'Hello, World!');
console.log(loroText.toString(doc, text));

// Work with map
const map = doc.getMap('data');
loroMap.insertString(doc, map, 'key', 'value');

// Commit and export
doc.commit('My changes');
const updates = doc.exportUpdates();

// Sync to another document
const doc2 = new loroDoc.Doc();
doc2.importUpdates(updates);
```

### Rust (with wasmtime)

```rust
use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store};

// Load and instantiate the component
let engine = Engine::new(&config)?;
let component = Component::from_file(&engine, "loro_wit.wasm")?;
let (loro, _) = LoroWorld::instantiate(&mut store, &component, &linker)?;

// Use the interfaces
let doc = loro.loro_crdt_loro_doc().doc().call_constructor(&mut store)?;
let text = loro.loro_crdt_loro_doc().doc().call_get_text(&mut store, doc, "content")?;
loro.loro_crdt_loro_text().call_insert(&mut store, doc, text, 0, "Hello!")?;
```

## WIT Interface Definition

See `crates/loro-wit/wit/loro.wit` for the full interface definition.

## Requirements

- Rust 1.85+ with `wasm32-wasip1` target
- [cargo-component](https://github.com/bytecodealliance/cargo-component)
- Node.js 18+ (for jco demo)
- npm or pnpm
