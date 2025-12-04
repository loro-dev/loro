//! Loro CRDT WebAssembly Component Demo with wasmtime
//!
//! This demo shows how to host and use the Loro CRDT library compiled as a
//! WebAssembly Component using wasmtime.

use anyhow::Result;
use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::preview2::{WasiCtx, WasiCtxBuilder, WasiView};

// Generate bindings for the Loro component
wasmtime::component::bindgen!({
    path: "../../../crates/loro-wit/wit",
    world: "loro-world",
    async: false,
});

struct MyState {
    ctx: WasiCtx,
    table: ResourceTable,
}

impl WasiView for MyState {
    fn ctx(&mut self) -> &mut WasiCtx {
        &mut self.ctx
    }
    fn table(&mut self) -> &mut ResourceTable {
        &mut self.table
    }
}

fn main() -> Result<()> {
    println!("=== Loro CRDT WebAssembly Component Demo (wasmtime) ===\n");

    // Configure the engine with component model support
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;

    // Load the component
    let component_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../target/wasm32-wasip1/release/loro_wit.wasm"
    );
    println!("Loading component from: {}\n", component_path);
    let component = Component::from_file(&engine, component_path)?;

    // Set up the linker with WASI
    let mut linker = Linker::new(&engine);
    wasmtime_wasi::preview2::command::add_to_linker(&mut linker)?;

    // Create the store with WASI context
    let wasi_ctx = WasiCtxBuilder::new()
        .inherit_stdio()
        .build();
    let state = MyState {
        ctx: wasi_ctx,
        table: ResourceTable::new(),
    };
    let mut store = Store::new(&engine, state);

    // Instantiate the component
    let (loro, _instance) = LoroWorld::instantiate(&mut store, &component, &linker)?;

    // Get the interfaces
    let doc_interface = &loro.loro_crdt_loro_doc();
    let text_interface = &loro.loro_crdt_loro_text();
    let map_interface = &loro.loro_crdt_loro_map();
    let list_interface = &loro.loro_crdt_loro_list();

    // Create a new document
    println!("1. Creating a new Loro document...");
    let doc = doc_interface.doc().call_constructor(&mut store)?;
    let peer_id = doc_interface.doc().call_peer_id(&mut store, doc)?;
    let is_empty = doc_interface.doc().call_is_empty(&mut store, doc)?;
    println!("   Document created with peer ID: {}", peer_id);
    println!("   Is empty: {}\n", is_empty);

    // Working with Text
    println!("2. Working with Text container...");
    let text_handle = doc_interface.doc().call_get_text(&mut store, doc, "my-text")?;
    text_interface.call_insert(&mut store, doc, text_handle, 0, "Hello, ")?;
    text_interface.call_insert(&mut store, doc, text_handle, 7, "World!")?;
    let text_content = text_interface.call_to_string(&mut store, doc, text_handle)?;
    let text_len_utf8 = text_interface.call_len_utf8(&mut store, doc, text_handle)?;
    let text_len_unicode = text_interface.call_len_unicode(&mut store, doc, text_handle)?;
    println!("   Text content: \"{}\"", text_content);
    println!("   Length (UTF-8): {}", text_len_utf8);
    println!("   Length (Unicode): {}\n", text_len_unicode);

    // Working with Map
    println!("3. Working with Map container...");
    let map_handle = doc_interface.doc().call_get_map(&mut store, doc, "my-map")?;
    map_interface.call_insert_string(&mut store, doc, map_handle, "name", "Loro")?;
    map_interface.call_insert_number(&mut store, doc, map_handle, "version", 1.10)?;
    map_interface.call_insert_bool(&mut store, doc, map_handle, "isAwesome", true)?;
    let keys = map_interface.call_keys(&mut store, doc, map_handle)?;
    let name = map_interface.call_get(&mut store, doc, map_handle, "name")?;
    let version = map_interface.call_get(&mut store, doc, map_handle, "version")?;
    let map_len = map_interface.call_len(&mut store, doc, map_handle)?;
    println!("   Keys: {:?}", keys);
    println!("   Name: {:?}", name);
    println!("   Version: {:?}", version);
    println!("   Map length: {}\n", map_len);

    // Working with List
    println!("4. Working with List container...");
    let list_handle = doc_interface.doc().call_get_list(&mut store, doc, "my-list")?;
    list_interface.call_push_string(&mut store, doc, list_handle, "first")?;
    list_interface.call_push_number(&mut store, doc, list_handle, 42.0)?;
    list_interface.call_insert_string(&mut store, doc, list_handle, 0, "zeroth")?;
    let list_len = list_interface.call_len(&mut store, doc, list_handle)?;
    let item0 = list_interface.call_get(&mut store, doc, list_handle, 0)?;
    let item1 = list_interface.call_get(&mut store, doc, list_handle, 1)?;
    let item2 = list_interface.call_get(&mut store, doc, list_handle, 2)?;
    println!("   List length: {}", list_len);
    println!("   Item 0: {:?}", item0);
    println!("   Item 1: {:?}", item1);
    println!("   Item 2: {:?}\n", item2);

    // Commit changes
    println!("5. Committing changes...");
    doc_interface.doc().call_commit(&mut store, doc, Some("Initial data setup"))?;
    println!("   Changes committed!\n");

    // Export and show document state
    println!("6. Document state:");
    let json_state = doc_interface.doc().call_to_json(&mut store, doc)?;
    println!("   JSON: {}\n", json_state);

    // Export updates
    println!("7. Exporting updates...");
    let updates = doc_interface.doc().call_export_updates(&mut store, doc)?;
    println!("   Updates size: {} bytes\n", updates.len());

    // Create a new document and import updates
    println!("8. Syncing with another document...");
    let doc2 = doc_interface.doc().call_constructor(&mut store)?;
    let peer_id2 = doc_interface.doc().call_peer_id(&mut store, doc2)?;
    println!("   Doc2 peer ID: {}", peer_id2);
    doc_interface.doc().call_import_updates(&mut store, doc2, &updates)?;
    let doc2_json = doc_interface.doc().call_to_json(&mut store, doc2)?;
    println!("   Doc2 after import: {}\n", doc2_json);

    // Fork the document
    println!("9. Forking document...");
    let forked = doc_interface.doc().call_fork(&mut store, doc)?;
    let forked_peer_id = doc_interface.doc().call_peer_id(&mut store, forked)?;
    println!("   Forked doc peer ID: {}", forked_peer_id);
    let forked_text = doc_interface.doc().call_get_text(&mut store, forked, "my-text")?;
    text_interface.call_insert(&mut store, forked, forked_text, 0, "[Forked] ")?;
    doc_interface.doc().call_commit(&mut store, forked, Some("Forked edit"))?;
    let forked_text_content = text_interface.call_to_string(&mut store, forked, forked_text)?;
    println!("   Forked text: \"{}\"\n", forked_text_content);

    println!("=== Demo Complete ===");

    Ok(())
}
