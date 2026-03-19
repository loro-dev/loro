use loro_common::ContainerID;
use loro_internal::{
    json::{JsonChange, JsonOp, JsonOpContent, JsonSchema, MapOp},
    loro::ExportMode,
    ContainerType, LoroDoc, LoroValue, ID,
};
use std::mem::ManuallyDrop;
use std::panic::{self, AssertUnwindSafe};

// Each synthetic change is a single map insert. The container/op type does not matter for this
// bug; we only need a tiny legal change graph with explicit deps.
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

// Construct the smallest DAG that can double-push a shared dependency during ensure_vv_for:
//
//   A
//  / \
// C   B
//
// where:
// - C depends on A
// - B depends on both A and C
//
// When B's deps iterate as [A, C], the DFS pushes A, then C, and then C pushes A again.
fn make_changes(peer_a: u64, peer_b: u64, peer_c: u64) -> [JsonChange; 3] {
    let a = map_insert_change(peer_a, vec![], 0, "a");
    let c = map_insert_change(peer_c, vec![ID::new(peer_a, 0)], 1, "c");
    let b = map_insert_change(peer_b, vec![ID::new(peer_a, 0), ID::new(peer_c, 0)], 2, "b");
    [a, c, b]
}

fn find_peer_ids_for_duplicate_stack_shape() -> (u64, u64, u64) {
    // The duplicate push only happens when B iterates deps as [A, C].
    // Frontiers with 2+ deps are backed by FxHashMap, so choose peers dynamically
    // instead of hard-coding a hasher-dependent order.
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

    panic!("failed to find peer ids where B iterates deps as [A, C]");
}

fn export_two_blob_minimal_graph(peer_a: u64, peer_b: u64, peer_c: u64) -> (Vec<u8>, Vec<u8>) {
    let [a, c, b] = make_changes(peer_a, peer_b, peer_c);

    // Blob 1 is a snapshot that already contains the shared dep A and the sibling C -> A.
    // Blob 2 is a plain update that adds only B -> [A, C].
    //
    // This is the smallest batch shape that still reaches the buggy detached import path:
    // one blob is not enough because import_batch([single_blob]) delegates to import().
    let snapshot_doc = doc_from_json_changes(vec![a.clone(), c.clone()]);
    let snapshot = snapshot_doc.export(ExportMode::Snapshot).unwrap();
    let snapshot_vv = snapshot_doc.oplog_vv();

    let full_doc = doc_from_json_changes(vec![a, c, b]);
    let update = full_doc.export(ExportMode::updates(&snapshot_vv)).unwrap();

    (snapshot, update)
}

fn import_batch_error(bytes: &[Vec<u8>]) -> Option<String> {
    // The current bug panics inside import_batch and would otherwise poison the doc on drop,
    // which aborts the test process. Catch the panic and suppress the drop path so the test can
    // report a normal assertion failure instead.
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

/// Minimal repro for the `ensure_vv_for` double-push bug:
///
/// - 3 changes total: `A`, `C -> A`, `B -> [A, C]`
/// - 2 blobs total: `Snapshot(A, C)` and `Update(B)`
///
/// `import_batch` needs at least 2 blobs because the 1-blob fast path delegates to `import`.
/// Sequential `import()` works because importing the snapshot computes/caches VVs for the partial
/// graph before `B` exists. `import_batch` imports both blobs while detached, so the final
/// checkout sees the full 3-node DAG with every `OnceCell` still empty.
#[test]
fn import_batch_panic_shared_dep_on_dag_node() {
    let (peer_a, peer_b, peer_c) = find_peer_ids_for_duplicate_stack_shape();
    let (snapshot, update) = export_two_blob_minimal_graph(peer_a, peer_b, peer_c);

    // The batch really is "2 blobs / 3 changes": the snapshot carries A and C, and the update
    // carries only B.
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

    // Sequential import works because the first import attaches state to the A <- C partial graph,
    // which computes VVs before B is added by the second import.
    {
        let doc = LoroDoc::new();
        doc.import(&snapshot).unwrap();
        doc.import(&update).unwrap();
    }

    // Batch import should also work. Without the ensure_vv_for guard, the final checkout traverses
    // B -> [A, C], pushes A twice, and panics on the second OnceCell::set().
    if let Some(err) = import_batch_error(&[snapshot, update]) {
        panic!("{err}");
    }
}
