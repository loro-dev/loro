use loro::{
    awareness::{Awareness, EphemeralStore},
    cursor::{Cursor, Side},
    LoroDoc, LoroValue, ToJson, ID,
};

const EXPECTED_JSON: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/snapshot.deep.json");
const RUST_UPDATES: &[u8] = include_bytes!("../../../loro-js/tests/fixtures/rust/updates.blob");
const TS_UPDATES: &[u8] = include_bytes!("../../../loro-js/tests/fixtures/rust/updates.ts.blob");
const TS_SNAPSHOT: &[u8] = include_bytes!("../../../loro-js/tests/fixtures/rust/snapshot.ts.blob");
const TS_RUNTIME_UPDATES: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/runtime-updates.ts.blob");
const TS_RUNTIME_SNAPSHOT: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/runtime-snapshot.ts.blob");
const TS_RUNTIME_EXPECTED_JSON: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/runtime.expected.json");
const TS_CONCURRENT_BASE: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/concurrent-base.ts.blob");
const TS_CONCURRENT_LEFT: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/concurrent-left.ts.blob");
const TS_CONCURRENT_RIGHT: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/concurrent-right.ts.blob");
const TS_CONCURRENT_EXPECTED_JSON: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/concurrent.expected.json");
const TS_FUGUE_LEFT: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/fugue-left.ts.blob");
const TS_FUGUE_RIGHT: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/fugue-right.ts.blob");
const TS_SHALLOW_SNAPSHOT: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/shallow.ts.blob");
const TS_CURSOR: &[u8] = include_bytes!("../../../loro-js/tests/fixtures/rust/cursor.ts.blob");
const TS_AWARENESS: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/awareness.ts.blob");
const TS_EPHEMERAL: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/ephemeral.ts.blob");

fn expected_json() -> serde_json::Value {
    serde_json::from_slice(EXPECTED_JSON).expect("valid expected JSON fixture")
}

#[test]
fn imports_typescript_reencoded_updates() {
    let expected = LoroDoc::new();
    expected
        .import(RUST_UPDATES)
        .expect("valid source FastUpdates fixture");
    let doc = LoroDoc::new();
    doc.import(TS_UPDATES)
        .expect("Rust should import TypeScript-encoded FastUpdates");
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        expected.get_deep_value().to_json_value()
    );
}

#[test]
fn imports_typescript_reencoded_snapshot() {
    let doc = LoroDoc::new();
    doc.import(TS_SNAPSHOT)
        .expect("Rust should import TypeScript-encoded FastSnapshot");
    assert_eq!(doc.get_deep_value().to_json_value(), expected_json());
}

#[test]
fn imports_typescript_runtime_updates() {
    let doc = LoroDoc::new();
    doc.import(TS_RUNTIME_UPDATES)
        .expect("Rust should import updates produced by the TypeScript runtime");
    let expected: serde_json::Value =
        serde_json::from_slice(TS_RUNTIME_EXPECTED_JSON).expect("valid runtime JSON fixture");
    assert_eq!(doc.get_deep_value().to_json_value(), expected);
}

#[test]
fn imports_typescript_runtime_snapshot() {
    let doc = LoroDoc::new();
    doc.import(TS_RUNTIME_SNAPSHOT)
        .expect("Rust should import a snapshot produced by the TypeScript runtime");
    let updates_doc = LoroDoc::new();
    updates_doc
        .import(TS_RUNTIME_UPDATES)
        .expect("Rust should import matching TypeScript updates");
    let expected: serde_json::Value =
        serde_json::from_slice(TS_RUNTIME_EXPECTED_JSON).expect("valid runtime JSON fixture");
    assert_eq!(doc.get_deep_value().to_json_value(), expected);
    assert_eq!(
        doc.get_text("text").to_delta(),
        updates_doc.get_text("text").to_delta()
    );
    assert_eq!(
        doc.get_text("text").get_richtext_value().to_json_value(),
        serde_json::json!([{ "insert": "b", "attributes": { "bold": true } }])
    );
}

#[test]
fn typescript_concurrent_updates_match_rust_in_both_import_orders() {
    let expected: serde_json::Value =
        serde_json::from_slice(TS_CONCURRENT_EXPECTED_JSON).expect("valid concurrent JSON fixture");
    let left_first = LoroDoc::new();
    left_first.import(TS_CONCURRENT_BASE).unwrap();
    left_first.import(TS_CONCURRENT_LEFT).unwrap();
    left_first.import(TS_CONCURRENT_RIGHT).unwrap();
    let right_first = LoroDoc::new();
    right_first.import(TS_CONCURRENT_BASE).unwrap();
    right_first.import(TS_CONCURRENT_RIGHT).unwrap();
    right_first.import(TS_CONCURRENT_LEFT).unwrap();

    assert_eq!(left_first.get_deep_value().to_json_value(), expected);
    assert_eq!(right_first.get_deep_value().to_json_value(), expected);
}

#[test]
fn typescript_fugue_updates_match_rust_in_both_import_orders() {
    let expected = serde_json::json!({ "text": "Hello World!" });
    let left_first = LoroDoc::new();
    left_first.import(TS_FUGUE_LEFT).unwrap();
    left_first.import(TS_FUGUE_RIGHT).unwrap();
    let right_first = LoroDoc::new();
    right_first.import(TS_FUGUE_RIGHT).unwrap();
    right_first.import(TS_FUGUE_LEFT).unwrap();

    assert_eq!(left_first.get_deep_value().to_json_value(), expected);
    assert_eq!(right_first.get_deep_value().to_json_value(), expected);
}

#[test]
fn imports_typescript_shallow_snapshot_and_checks_out_its_root() {
    let doc = LoroDoc::new();
    doc.import(TS_SHALLOW_SNAPSHOT)
        .expect("Rust should import a TypeScript shallow snapshot");

    assert!(doc.is_shallow());
    assert_eq!(doc.shallow_since_vv().get(&77).copied(), Some(4));
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        serde_json::json!({ "text": "0123456789" })
    );

    let root = doc.shallow_since_frontiers();
    doc.checkout(&root)
        .expect("the retained shallow root should be checkout-able");
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        serde_json::json!({ "text": "01234" })
    );
}

#[test]
fn decodes_typescript_cursor_with_rust() {
    let cursor = Cursor::decode(TS_CURSOR).expect("Rust should decode a TypeScript cursor");
    assert_eq!(cursor.id, Some(ID::new(99, 1)));
    assert_eq!(cursor.container.to_string(), "cid:root-text:Text");
    assert_eq!(cursor.side, Side::Middle);
}

#[allow(deprecated)]
#[test]
fn imports_typescript_awareness_with_rust() {
    let mut awareness = Awareness::new(456, 30_000);
    let (updated, added) = awareness
        .try_apply(TS_AWARENESS)
        .expect("Rust should decode TypeScript awareness data");
    assert!(updated.is_empty());
    assert_eq!(added, vec![123]);
    let state = &awareness.get_all_states().get(&123).unwrap().state;
    assert_eq!(
        state.to_json_value(),
        serde_json::json!({ "status": "typing", "position": 3 })
    );
}

#[test]
fn imports_typescript_ephemeral_state_with_rust() {
    let store = EphemeralStore::new(i64::MAX);
    store
        .apply(TS_EPHEMERAL)
        .expect("Rust should decode TypeScript ephemeral data");
    assert_eq!(store.get("cursor"), Some(LoroValue::from(7)));
}
