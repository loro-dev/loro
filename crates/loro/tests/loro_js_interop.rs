use loro::{
    awareness::{Awareness, EphemeralStore},
    cursor::{Cursor, Side},
    ExportMode, LoroDoc, LoroValue, ToJson, TreeID, ID,
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
const TS_TREE_MOVE_UPDATES: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/tree-move-updates.ts.blob");
const TS_TREE_MOVE_SNAPSHOT: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/tree-move-snapshot.ts.blob");
const TS_TREE_MOVE_SHALLOW_SNAPSHOT: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/tree-move-shallow.ts.blob");
const TS_LEGACY_TREE_MOVE_SNAPSHOT: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/legacy-tree-move-snapshot.ts.blob");
const TS_LEGACY_TREE_MOVE_SHALLOW_SNAPSHOT: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/legacy-tree-move-shallow.ts.blob");
const TS_RICHTEXT_BASE: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/richtext-base.ts.blob");
const TS_RICHTEXT_LEFT: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/richtext-left.ts.blob");
const TS_RICHTEXT_RIGHT: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/richtext-right.ts.blob");
const TS_RICHTEXT_SNAPSHOT: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/richtext-snapshot.ts.blob");
const TS_RICHTEXT_EXPECTED_JSON: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/richtext.expected.json");
const TS_MOVABLE_SNAPSHOT: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/movable-snapshot.ts.blob");
const RUST_MOVABLE_UPDATES: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/movable-snapshot-updates.blob");
const RUST_MOVABLE_PEER3: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/movable-snapshot-peer3.blob");
const RUST_MOVABLE_PEER4: &[u8] =
    include_bytes!("../../../loro-js/tests/fixtures/rust/movable-snapshot-peer4.blob");
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

/// A Quill delta with null attributes dropped and equal neighbors merged, so
/// deltas compare by content rather than by how each runtime splits them.
fn normalized_delta(value: serde_json::Value) -> serde_json::Value {
    let mut output: Vec<serde_json::Value> = Vec::new();
    for item in value.as_array().expect("a delta is an array") {
        let insert = item["insert"].as_str().expect("a text insert").to_string();
        let attributes: serde_json::Map<String, serde_json::Value> = item
            .get("attributes")
            .and_then(|attributes| attributes.as_object())
            .map(|attributes| {
                attributes
                    .iter()
                    .filter(|(_, value)| !value.is_null())
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(previous) = output.last_mut() {
            let previous_attributes = previous
                .get("attributes")
                .cloned()
                .unwrap_or(serde_json::Value::Object(Default::default()));
            if previous_attributes == serde_json::Value::Object(attributes.clone()) {
                let merged = format!("{}{}", previous["insert"].as_str().unwrap(), insert);
                previous["insert"] = merged.into();
                continue;
            }
        }
        let mut entry = serde_json::json!({ "insert": insert });
        entry["attributes"] = serde_json::Value::Object(attributes);
        output.push(entry);
    }
    serde_json::Value::Array(output)
}

fn richtext_delta(doc: &LoroDoc) -> serde_json::Value {
    normalized_delta(doc.get_text("text").get_richtext_value().to_json_value())
}

#[test]
fn imports_typescript_rich_text_with_anchor_positions() {
    let expected: serde_json::Value =
        serde_json::from_slice(TS_RICHTEXT_EXPECTED_JSON).expect("valid rich-text fixture");
    let expected_delta = normalized_delta(expected["delta"].clone());

    let doc = LoroDoc::new();
    for update in [TS_RICHTEXT_BASE, TS_RICHTEXT_LEFT, TS_RICHTEXT_RIGHT] {
        doc.import(update)
            .expect("Rust should import TypeScript rich-text updates");
    }
    assert_eq!(richtext_delta(&doc), expected_delta);

    let snapshot = LoroDoc::new();
    snapshot
        .import(TS_RICHTEXT_SNAPSHOT)
        .expect("Rust should import a TypeScript rich-text snapshot");
    assert_eq!(richtext_delta(&snapshot), expected_delta);
    let frontiers: Vec<ID> = expected["base"]["frontiers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| {
            ID::new(
                id["peer"].as_str().unwrap().parse().unwrap(),
                id["counter"].as_i64().unwrap() as i32,
            )
        })
        .collect();
    snapshot
        .checkout(&frontiers.into())
        .expect("the snapshot history should check out");
    assert_eq!(
        richtext_delta(&snapshot),
        normalized_delta(expected["base"]["delta"].clone())
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

fn assert_same_tree_state(actual: &LoroDoc, expected: &LoroDoc) {
    assert_eq!(
        actual.get_deep_value().to_json_value(),
        expected.get_deep_value().to_json_value()
    );
    let actual = actual.get_tree("tree");
    let expected = expected.get_tree("tree");
    let nodes = expected.get_nodes(true);
    assert_eq!(
        format!("{:?}", actual.get_nodes(true)),
        format!("{nodes:?}")
    );
    for node in nodes {
        assert_eq!(
            actual.is_node_deleted(&node.id).unwrap(),
            expected.is_node_deleted(&node.id).unwrap()
        );
        assert_eq!(
            actual.get_last_move_id(&node.id),
            expected.get_last_move_id(&node.id),
            "last move id of {:?}",
            node.id
        );
    }
}

#[test]
fn imports_typescript_tree_snapshots_after_moves() {
    let expected = LoroDoc::new();
    expected.import(TS_TREE_MOVE_UPDATES).unwrap();

    let snapshot = LoroDoc::new();
    snapshot
        .import(TS_TREE_MOVE_SNAPSHOT)
        .expect("Rust should import a TypeScript snapshot with moved tree nodes");
    assert_same_tree_state(&snapshot, &expected);
    let reencoded = LoroDoc::new();
    reencoded
        .import(&snapshot.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_same_tree_state(&reencoded, &expected);

    let shallow = LoroDoc::new();
    shallow
        .import(TS_TREE_MOVE_SHALLOW_SNAPSHOT)
        .expect("Rust should import a TypeScript shallow snapshot with moved tree nodes");
    assert!(shallow.is_shallow());
    assert_same_tree_state(&shallow, &expected);
    let root = shallow.shallow_since_frontiers();
    shallow.checkout(&root).unwrap();
    expected.checkout(&root).unwrap();
    assert_same_tree_state(&shallow, &expected);
}

/// loro.js <= 0.2.0 wrote tree siblings in creation order (loro-dev/loro#1088).
#[test]
fn imports_legacy_typescript_tree_snapshots_with_unordered_siblings() {
    for bytes in [
        TS_LEGACY_TREE_MOVE_SNAPSHOT,
        TS_LEGACY_TREE_MOVE_SHALLOW_SNAPSHOT,
    ] {
        let doc = LoroDoc::new();
        doc.import(bytes).unwrap();
        let tree = doc.get_tree("x");
        let root = tree.roots()[0];
        assert_eq!(
            tree.children(root).unwrap(),
            vec![TreeID::new(1, 2), TreeID::new(1, 1)]
        );
        assert_eq!(
            doc.get_deep_value().to_json_value(),
            serde_json::json!({ "x": [{
                "id": "0@1", "parent": null, "index": 0, "fractional_index": "80", "meta": {},
                "children": [
                    { "id": "2@1", "parent": "0@1", "index": 0, "fractional_index": "7F80",
                      "meta": {}, "children": [] },
                    { "id": "1@1", "parent": "0@1", "index": 1, "fractional_index": "80",
                      "meta": {}, "children": [] },
                ],
            }] })
        );
        let reencoded = LoroDoc::new();
        reencoded
            .import(&doc.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        assert_eq!(
            reencoded.get_deep_value().to_json_value(),
            doc.get_deep_value().to_json_value()
        );
    }
}

fn movable_list_metadata(doc: &LoroDoc) -> Vec<(Option<u64>, Option<u64>, Option<u64>)> {
    let list = doc.get_movable_list("list");
    (0..list.len())
        .map(|index| {
            (
                list.get_creator_at(index),
                list.get_last_mover_at(index),
                list.get_last_editor_at(index),
            )
        })
        .collect()
}

/// loro.js writes a MovableList snapshot with its list item, element and
/// last-set IDs, so later edits converge with a replica that has the history.
#[test]
fn imports_typescript_movable_list_snapshot_with_its_metadata() {
    let expected = LoroDoc::new();
    expected.import(RUST_MOVABLE_UPDATES).unwrap();
    let doc = LoroDoc::new();
    doc.import(TS_MOVABLE_SNAPSHOT)
        .expect("Rust should import a MovableList snapshot produced by loro.js");
    assert_eq!(doc.get_deep_value(), expected.get_deep_value());
    assert_eq!(movable_list_metadata(&doc), movable_list_metadata(&expected));

    for update in [RUST_MOVABLE_PEER4, RUST_MOVABLE_PEER3] {
        doc.import(update).unwrap();
        expected.import(update).unwrap();
    }
    assert_eq!(doc.get_deep_value(), expected.get_deep_value());
    assert_eq!(movable_list_metadata(&doc), movable_list_metadata(&expected));
}
