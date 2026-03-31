use loro_common::ContainerID;
use loro_internal::{
    json::{JsonChange, JsonOp, JsonOpContent, JsonSchema, MapOp},
    loro::ExportMode,
    ContainerType, LoroDoc, LoroValue, ID,
};
use std::mem::ManuallyDrop;
use std::panic::{self, AssertUnwindSafe};

// This shape is not produced by local frontiers, which are already shrunk to a minimal set.
// It is still accepted by the public JSON import API, so batch import must not panic when it
// encounters a causally valid history with redundant dependencies.
fn map_insert_change(peer: u64, deps: Vec<ID>, lamport: u32, key: &str) -> JsonChange {
    JsonChange {
        id: ID::new(peer, 0),
        timestamp: 0,
        deps,
        lamport,
        msg: None,
        ops: vec![JsonOp {
            content: JsonOpContent::Map(MapOp::Insert {
                key: key.into(),
                value: LoroValue::from(key),
            }),
            container: ContainerID::new_root("map", ContainerType::Map),
            counter: 0,
        }],
    }
}

fn doc_from_json_changes(changes: Vec<JsonChange>) -> LoroDoc {
    let doc = LoroDoc::new();
    doc.import_json_updates(JsonSchema {
        schema_version: 1,
        start_version: Default::default(),
        peers: None,
        changes,
    })
    .unwrap();
    doc
}

// Build the smallest imported graph that can queue the same shared dependency twice:
//
//   A
//  / \
// C   B
//
// where B imports with deps [A, C].
fn make_changes(peer_a: u64, peer_b: u64, peer_c: u64) -> [JsonChange; 3] {
    let a = map_insert_change(peer_a, vec![], 0, "a");
    let c = map_insert_change(peer_c, vec![ID::new(peer_a, 0)], 1, "c");
    let b = map_insert_change(peer_b, vec![ID::new(peer_a, 0), ID::new(peer_c, 0)], 2, "b");
    [a, c, b]
}

fn find_peer_ids_for_duplicate_stack_shape() -> (u64, u64, u64) {
    // Frontiers with 2+ deps iterate through FxHashMap, so choose peer ids dynamically until
    // the imported deps show up in the duplicate-push order [A, C].
    for peer_a in 1..16 {
        for peer_c in 1..16 {
            if peer_c == peer_a {
                continue;
            }

            for peer_b in 1..16 {
                if peer_b == peer_a || peer_b == peer_c {
                    continue;
                }

                let [a, c, b] = make_changes(peer_a, peer_b, peer_c);
                let doc = doc_from_json_changes(vec![a, c, b]);
                let deps = doc
                    .oplog()
                    .lock()
                    .unwrap()
                    .get_change_at(ID::new(peer_b, 0))
                    .unwrap()
                    .deps()
                    .to_vec();

                if deps == vec![ID::new(peer_a, 0), ID::new(peer_c, 0)] {
                    return (peer_a, peer_b, peer_c);
                }
            }
        }
    }

    panic!("failed to find peer ids where imported deps iterate as [A, C]");
}

fn export_two_blob_minimal_graph(peer_a: u64, peer_b: u64, peer_c: u64) -> (Vec<u8>, Vec<u8>) {
    let [a, c, b] = make_changes(peer_a, peer_b, peer_c);

    let snapshot_doc = doc_from_json_changes(vec![a.clone(), c.clone()]);
    let snapshot = snapshot_doc.export(ExportMode::Snapshot).unwrap();
    let snapshot_vv = snapshot_doc.oplog_vv();

    let full_doc = doc_from_json_changes(vec![a, c, b]);
    let update = full_doc.export(ExportMode::updates(&snapshot_vv)).unwrap();

    (snapshot, update)
}

fn import_batch_error(bytes: &[Vec<u8>]) -> Option<String> {
    // If batch import panics mid-way, dropping the partially imported doc can abort the whole
    // test process. Catch the unwind and suppress the drop path so regressions fail normally.
    let hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));

    let mut doc = ManuallyDrop::new(LoroDoc::new());
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        (&*doc).import_batch(bytes).unwrap();
    }));

    panic::set_hook(hook);
    match result {
        Ok(()) => {
            unsafe { ManuallyDrop::drop(&mut doc) };
            None
        }
        Err(_) => Some("import_batch panicked while computing version vectors".to_string()),
    }
}

#[test]
fn import_batch_handles_shared_dependency_from_imported_json() {
    let (peer_a, peer_b, peer_c) = find_peer_ids_for_duplicate_stack_shape();
    let (snapshot, update) = export_two_blob_minimal_graph(peer_a, peer_b, peer_c);

    assert_eq!(
        LoroDoc::decode_import_blob_meta(&snapshot, false)
            .unwrap()
            .change_num,
        2
    );
    assert_eq!(
        LoroDoc::decode_import_blob_meta(&update, false)
            .unwrap()
            .change_num,
        1
    );

    {
        let doc = LoroDoc::new();
        doc.import(&snapshot).unwrap();
        doc.import(&update).unwrap();
    }

    if let Some(err) = import_batch_error(&[snapshot.clone(), update.clone()]) {
        panic!("{err}");
    }

    let expected_doc = doc_from_json_changes(Vec::from(make_changes(peer_a, peer_b, peer_c)));
    let batch_doc = LoroDoc::new();
    batch_doc.import_batch(&[snapshot, update]).unwrap();
    assert_eq!(batch_doc.get_deep_value(), expected_doc.get_deep_value());
}
