//! WebAssembly Component Model (WIT) bindings for Loro CRDT
//!
//! This crate provides WIT-based bindings for the Loro CRDT library,
//! enabling use with the WebAssembly Component Model and tools like
//! jco (JavaScript) and wasmtime.

#[allow(warnings)]
mod bindings;

use std::cell::RefCell;
use std::collections::HashMap;

use bindings::exports::loro::crdt::loro_doc::{
    Doc as WitDoc, DocBorrow, GuestDoc, ListHandle, MapHandle, TextHandle,
};
use loro::{
    ExportMode, LoroDoc, LoroList as InnerLoroList, LoroMap as InnerLoroMap,
    LoroText as InnerLoroText, LoroValue, ValueOrContainer,
};

/// Global storage for container handles
/// This maps handle IDs to their container names, allowing us to
/// reconstruct containers from handles
struct HandleStorage {
    next_id: u64,
    text_names: HashMap<u64, String>,
    map_names: HashMap<u64, String>,
    list_names: HashMap<u64, String>,
}

impl HandleStorage {
    fn new() -> Self {
        Self {
            next_id: 1,
            text_names: HashMap::new(),
            map_names: HashMap::new(),
            list_names: HashMap::new(),
        }
    }

    fn register_text(&mut self, name: String) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.text_names.insert(id, name);
        id
    }

    fn register_map(&mut self, name: String) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.map_names.insert(id, name);
        id
    }

    fn register_list(&mut self, name: String) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.list_names.insert(id, name);
        id
    }

    fn get_text_name(&self, id: u64) -> Option<String> {
        self.text_names.get(&id).cloned()
    }

    fn get_map_name(&self, id: u64) -> Option<String> {
        self.map_names.get(&id).cloned()
    }

    fn get_list_name(&self, id: u64) -> Option<String> {
        self.list_names.get(&id).cloned()
    }
}

thread_local! {
    static HANDLE_STORAGE: RefCell<HandleStorage> = RefCell::new(HandleStorage::new());
}

/// The Loro document resource implementation
pub struct Doc {
    inner: LoroDoc,
}

impl GuestDoc for Doc {
    fn new() -> Self {
        Self {
            inner: LoroDoc::new(),
        }
    }

    fn new_with_peer_id(peer_id: u64) -> WitDoc {
        let doc = LoroDoc::new();
        doc.set_peer_id(peer_id).expect("Failed to set peer ID");
        WitDoc::new(Self { inner: doc })
    }

    fn peer_id(&self) -> u64 {
        self.inner.peer_id()
    }

    fn get_text(&self, name: String) -> TextHandle {
        // Get or create the text container to ensure it exists
        let cloned_name = name.clone();
        let _ = self.inner.get_text(cloned_name);
        let id = HANDLE_STORAGE.with(|storage| storage.borrow_mut().register_text(name));
        TextHandle { id }
    }

    fn get_map(&self, name: String) -> MapHandle {
        let cloned_name = name.clone();
        let _ = self.inner.get_map(cloned_name);
        let id = HANDLE_STORAGE.with(|storage| storage.borrow_mut().register_map(name));
        MapHandle { id }
    }

    fn get_list(&self, name: String) -> ListHandle {
        let cloned_name = name.clone();
        let _ = self.inner.get_list(cloned_name);
        let id = HANDLE_STORAGE.with(|storage| storage.borrow_mut().register_list(name));
        ListHandle { id }
    }

    fn commit(&self, message: Option<String>) {
        if let Some(msg) = message {
            self.inner.commit_with(
                loro::CommitOptions::default().commit_msg(&msg),
            );
        } else {
            self.inner.commit();
        }
    }

    fn export_updates(&self) -> Vec<u8> {
        self.inner
            .export(ExportMode::all_updates())
            .unwrap_or_default()
    }

    fn export_snapshot(&self) -> Vec<u8> {
        self.inner.export(ExportMode::Snapshot).unwrap_or_default()
    }

    fn import_updates(&self, bytes: Vec<u8>) -> Result<(), String> {
        self.inner
            .import(&bytes)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn to_json(&self) -> String {
        serde_json::to_string(&self.inner.get_deep_value()).unwrap_or_else(|_| "{}".to_string())
    }

    fn is_empty(&self) -> bool {
        // Check if the document has no operations by checking export size
        self.inner.export(ExportMode::all_updates())
            .map(|v| v.is_empty())
            .unwrap_or(true)
    }

    fn fork(&self) -> WitDoc {
        WitDoc::new(Self {
            inner: self.inner.fork(),
        })
    }
}

// Helper functions to get containers from handles
fn get_text_from_handle(doc: &Doc, handle: &TextHandle) -> Option<InnerLoroText> {
    HANDLE_STORAGE.with(|storage| {
        storage
            .borrow()
            .get_text_name(handle.id)
            .map(|name| doc.inner.get_text(name))
    })
}

fn get_map_from_handle(doc: &Doc, handle: &MapHandle) -> Option<InnerLoroMap> {
    HANDLE_STORAGE.with(|storage| {
        storage
            .borrow()
            .get_map_name(handle.id)
            .map(|name| doc.inner.get_map(name))
    })
}

fn get_list_from_handle(doc: &Doc, handle: &ListHandle) -> Option<InnerLoroList> {
    HANDLE_STORAGE.with(|storage| {
        storage
            .borrow()
            .get_list_name(handle.id)
            .map(|name| doc.inner.get_list(name))
    })
}

/// Convert ValueOrContainer to JSON string
fn value_or_container_to_json(v: ValueOrContainer) -> String {
    let loro_value = v.get_deep_value();
    serde_json::to_string(&loro_value).unwrap_or_default()
}

/// Main component struct that implements all interfaces
struct LoroComponent;

impl bindings::exports::loro::crdt::loro_doc::Guest for LoroComponent {
    type Doc = Doc;
}

impl bindings::exports::loro::crdt::loro_text::Guest for LoroComponent {
    fn insert(
        doc: DocBorrow<'_>,
        handle: TextHandle,
        pos: u32,
        text: String,
    ) -> Result<(), String> {
        let doc = doc.get::<Doc>();
        let text_container =
            get_text_from_handle(doc, &handle).ok_or_else(|| "Invalid text handle".to_string())?;
        text_container
            .insert(pos as usize, &text)
            .map_err(|e| e.to_string())
    }

    fn delete(
        doc: DocBorrow<'_>,
        handle: TextHandle,
        pos: u32,
        len: u32,
    ) -> Result<(), String> {
        let doc = doc.get::<Doc>();
        let text_container =
            get_text_from_handle(doc, &handle).ok_or_else(|| "Invalid text handle".to_string())?;
        text_container
            .delete(pos as usize, len as usize)
            .map_err(|e| e.to_string())
    }

    fn to_string(doc: DocBorrow<'_>, handle: TextHandle) -> String {
        let doc = doc.get::<Doc>();
        get_text_from_handle(doc, &handle)
            .map(|t| t.to_string())
            .unwrap_or_default()
    }

    fn len_utf8(doc: DocBorrow<'_>, handle: TextHandle) -> u32 {
        let doc = doc.get::<Doc>();
        get_text_from_handle(doc, &handle)
            .map(|t| t.len_utf8() as u32)
            .unwrap_or(0)
    }

    fn len_unicode(doc: DocBorrow<'_>, handle: TextHandle) -> u32 {
        let doc = doc.get::<Doc>();
        get_text_from_handle(doc, &handle)
            .map(|t| t.len_unicode() as u32)
            .unwrap_or(0)
    }
}

impl bindings::exports::loro::crdt::loro_map::Guest for LoroComponent {
    fn insert_string(
        doc: DocBorrow<'_>,
        handle: MapHandle,
        key: String,
        value: String,
    ) -> Result<(), String> {
        let doc = doc.get::<Doc>();
        let map_container =
            get_map_from_handle(doc, &handle).ok_or_else(|| "Invalid map handle".to_string())?;
        map_container
            .insert(&key, value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn insert_number(
        doc: DocBorrow<'_>,
        handle: MapHandle,
        key: String,
        value: f64,
    ) -> Result<(), String> {
        let doc = doc.get::<Doc>();
        let map_container =
            get_map_from_handle(doc, &handle).ok_or_else(|| "Invalid map handle".to_string())?;
        map_container
            .insert(&key, value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn insert_bool(
        doc: DocBorrow<'_>,
        handle: MapHandle,
        key: String,
        value: bool,
    ) -> Result<(), String> {
        let doc = doc.get::<Doc>();
        let map_container =
            get_map_from_handle(doc, &handle).ok_or_else(|| "Invalid map handle".to_string())?;
        map_container
            .insert(&key, value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn insert_null(doc: DocBorrow<'_>, handle: MapHandle, key: String) -> Result<(), String> {
        let doc = doc.get::<Doc>();
        let map_container =
            get_map_from_handle(doc, &handle).ok_or_else(|| "Invalid map handle".to_string())?;
        map_container
            .insert(&key, LoroValue::Null)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn get(doc: DocBorrow<'_>, handle: MapHandle, key: String) -> Option<String> {
        let doc = doc.get::<Doc>();
        let map_container = get_map_from_handle(doc, &handle)?;
        map_container
            .get(&key)
            .map(value_or_container_to_json)
    }

    fn delete(doc: DocBorrow<'_>, handle: MapHandle, key: String) -> Result<(), String> {
        let doc = doc.get::<Doc>();
        let map_container =
            get_map_from_handle(doc, &handle).ok_or_else(|| "Invalid map handle".to_string())?;
        map_container.delete(&key).map_err(|e| e.to_string())
    }

    fn contains(doc: DocBorrow<'_>, handle: MapHandle, key: String) -> bool {
        let doc = doc.get::<Doc>();
        get_map_from_handle(doc, &handle)
            .and_then(|m| m.get(&key))
            .is_some()
    }

    fn keys(doc: DocBorrow<'_>, handle: MapHandle) -> Vec<String> {
        let doc = doc.get::<Doc>();
        get_map_from_handle(doc, &handle)
            .map(|m| m.keys().map(|s| s.to_string()).collect())
            .unwrap_or_default()
    }

    fn len(doc: DocBorrow<'_>, handle: MapHandle) -> u32 {
        let doc = doc.get::<Doc>();
        get_map_from_handle(doc, &handle)
            .map(|m| m.len() as u32)
            .unwrap_or(0)
    }
}

impl bindings::exports::loro::crdt::loro_list::Guest for LoroComponent {
    fn insert_string(
        doc: DocBorrow<'_>,
        handle: ListHandle,
        pos: u32,
        value: String,
    ) -> Result<(), String> {
        let doc = doc.get::<Doc>();
        let list_container =
            get_list_from_handle(doc, &handle).ok_or_else(|| "Invalid list handle".to_string())?;
        list_container
            .insert(pos as usize, value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn insert_number(
        doc: DocBorrow<'_>,
        handle: ListHandle,
        pos: u32,
        value: f64,
    ) -> Result<(), String> {
        let doc = doc.get::<Doc>();
        let list_container =
            get_list_from_handle(doc, &handle).ok_or_else(|| "Invalid list handle".to_string())?;
        list_container
            .insert(pos as usize, value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn insert_bool(
        doc: DocBorrow<'_>,
        handle: ListHandle,
        pos: u32,
        value: bool,
    ) -> Result<(), String> {
        let doc = doc.get::<Doc>();
        let list_container =
            get_list_from_handle(doc, &handle).ok_or_else(|| "Invalid list handle".to_string())?;
        list_container
            .insert(pos as usize, value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn insert_null(doc: DocBorrow<'_>, handle: ListHandle, pos: u32) -> Result<(), String> {
        let doc = doc.get::<Doc>();
        let list_container =
            get_list_from_handle(doc, &handle).ok_or_else(|| "Invalid list handle".to_string())?;
        list_container
            .insert(pos as usize, LoroValue::Null)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn push_string(doc: DocBorrow<'_>, handle: ListHandle, value: String) -> Result<(), String> {
        let doc = doc.get::<Doc>();
        let list_container =
            get_list_from_handle(doc, &handle).ok_or_else(|| "Invalid list handle".to_string())?;
        list_container
            .push(value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn push_number(doc: DocBorrow<'_>, handle: ListHandle, value: f64) -> Result<(), String> {
        let doc = doc.get::<Doc>();
        let list_container =
            get_list_from_handle(doc, &handle).ok_or_else(|| "Invalid list handle".to_string())?;
        list_container
            .push(value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn get(doc: DocBorrow<'_>, handle: ListHandle, index: u32) -> Option<String> {
        let doc = doc.get::<Doc>();
        let list_container = get_list_from_handle(doc, &handle)?;
        list_container
            .get(index as usize)
            .map(value_or_container_to_json)
    }

    fn delete(doc: DocBorrow<'_>, handle: ListHandle, index: u32) -> Result<(), String> {
        let doc = doc.get::<Doc>();
        let list_container =
            get_list_from_handle(doc, &handle).ok_or_else(|| "Invalid list handle".to_string())?;
        list_container
            .delete(index as usize, 1)
            .map_err(|e| e.to_string())
    }

    fn len(doc: DocBorrow<'_>, handle: ListHandle) -> u32 {
        let doc = doc.get::<Doc>();
        get_list_from_handle(doc, &handle)
            .map(|l| l.len() as u32)
            .unwrap_or(0)
    }
}

// Export the component
bindings::export!(LoroComponent with_types_in bindings);
