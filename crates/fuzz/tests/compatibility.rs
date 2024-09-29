#![allow(deprecated)]
use std::sync::Arc;

use loro::{ToJson as _, ID};
use loro_016::ToJson as _;

#[test]
fn updates_with_commit_message_can_be_imported_to_016() {
    let doc1 = loro::LoroDoc::new();
    {
        let text = doc1.get_text("text");
        text.insert(0, "Hello, World!").unwrap();
        doc1.set_next_commit_message("Initial text insertion");
        doc1.commit();

        let tree = doc1.get_tree("tree");
        let root = tree.create(None).unwrap();
        doc1.set_next_commit_message("Added tree structure");
        doc1.commit();

        doc1.set_next_commit_message("Modified text");
        text.delete(0, 5).unwrap();
        text.insert(7, "Loro").unwrap();
        doc1.commit();

        doc1.set_next_commit_message("Added another child to tree");
        tree.create(root).unwrap();
        doc1.commit();
    }

    let doc2 = loro_016::LoroDoc::new();
    doc2.import(&doc1.export_snapshot()).unwrap();
    assert_eq!(
        doc2.get_text("text").to_string(),
        doc1.get_text("text").to_string()
    );

    assert_eq!(
        doc2.get_tree("tree")
            .nodes()
            .into_iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>(),
        doc1.get_tree("tree")
            .nodes()
            .into_iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
    );

    {
        doc2.get_text("text").insert(0, "123").unwrap();
        doc1.import(&doc2.export_from(&Default::default())).unwrap();
    }

    let doc3 = loro::LoroDoc::new();
    doc3.import(&doc1.export(loro::ExportMode::Snapshot))
        .unwrap();
    let change_from_2 = doc3.get_change(ID::new(doc2.peer_id(), 0)).unwrap();
    assert_eq!(change_from_2.len, 3);
    assert_eq!(doc3.get_deep_value(), doc1.get_deep_value());
    assert_eq!(
        doc3.get_change(ID::new(doc1.peer_id(), 0))
            .unwrap()
            .message(),
        "Initial text insertion"
    );
}

#[test]
fn snapshot_from_016_can_be_imported_in_cur_version() {
    // Create a LoroDoc using loro-016
    let doc_016 = loro_016::LoroDoc::new();
    doc_016.set_peer_id(1).unwrap();

    // Perform some operations on doc_016
    {
        let text = doc_016.get_text("text");
        text.insert(0, "Hello, Loro!").unwrap();
        doc_016.commit();

        let map = doc_016.get_map("map");
        map.insert("key", "value").unwrap();
        doc_016.commit();

        let list = doc_016.get_list("list");
        list.push(1).unwrap();
        list.push(2).unwrap();
        list.push(3).unwrap();
        doc_016.commit();
    }

    // Export a snapshot from doc_016
    let snapshot_016 = doc_016.export_snapshot();

    // Create a new LoroDoc using the current version
    let doc_current = loro::LoroDoc::new();

    // Import the snapshot from loro-016 into the current version
    doc_current.import(&snapshot_016).unwrap();

    // Verify that the imported data matches the original
    assert_eq!(
        doc_current.get_deep_value().to_json(),
        doc_016.get_deep_value().to_json()
    );

    // Perform additional operations on the current version doc
    {
        let text = doc_current.get_text("text");
        text.insert(11, " CRDT").unwrap();
        doc_current.commit();

        let map = doc_current.get_map("map");
        map.insert("new_key", "new_value").unwrap();
        doc_current.commit();

        let list = doc_current.get_list("list");
        list.push(4).unwrap();
        doc_current.commit();
    }

    // Verify that new operations work correctly
    assert_eq!(
        doc_current.get_text("text").to_string(),
        "Hello, Loro CRDT!"
    );
    assert_eq!(
        doc_current
            .get_map("map")
            .get("new_key")
            .unwrap()
            .into_value()
            .unwrap(),
        loro::LoroValue::String(Arc::new("new_value".into()))
    );
    assert_eq!(doc_current.get_list("list").len(), 4);

    // Export a snapshot from the current version
    let snapshot_current = doc_current.export_snapshot();

    // Create another LoroDoc using loro-016 and import the snapshot from the current version
    let doc_016_reimport = loro_016::LoroDoc::new();
    doc_016_reimport.import(&snapshot_current).unwrap();

    // Verify that the reimported data in loro-016 matches the current version
    assert_eq!(
        doc_016_reimport.get_deep_value().to_json(),
        doc_current.get_deep_value().to_json()
    );
}
