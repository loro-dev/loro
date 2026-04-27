#![no_main]
use libfuzzer_sys::fuzz_target;
use loro_internal::version::Frontiers;
use loro_internal::{LoroDoc, LoroError, LoroValue, TreeParentId, VersionVector};

struct DocSnapshot {
    value: LoroValue,
    oplog_vv: VersionVector,
    oplog_frontiers: Frontiers,
    state_frontiers: Frontiers,
}

fn seed_doc() -> LoroDoc {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(0xF00D).unwrap();

    let map = doc.get_map("map");
    map.insert("seed", "value").unwrap();
    map.insert("number", 42).unwrap();

    let list = doc.get_list("list");
    list.push("head").unwrap();
    list.push(7).unwrap();

    let text = doc.get_text("text");
    text.insert_unicode(0, "initial text").unwrap();

    let tree = doc.get_tree("tree");
    let node = tree.create(TreeParentId::Root).unwrap();
    tree.get_meta(node).unwrap().insert("kind", "seed").unwrap();

    doc.commit_then_renew();
    doc
}

fn snapshot(doc: &LoroDoc) -> DocSnapshot {
    DocSnapshot {
        value: doc.get_deep_value(),
        oplog_vv: doc.oplog_vv(),
        oplog_frontiers: doc.oplog_frontiers(),
        state_frontiers: doc.state_frontiers(),
    }
}

fn assert_unchanged(doc: &LoroDoc, before: &DocSnapshot) {
    assert_eq!(doc.get_deep_value(), before.value);
    assert_eq!(doc.oplog_vv(), before.oplog_vv);
    assert_eq!(doc.oplog_frontiers(), before.oplog_frontiers);
    assert_eq!(doc.state_frontiers(), before.state_frontiers);
}

fn is_prefix_import_error(error: &LoroError) -> bool {
    matches!(error, LoroError::ImportUpdatesThatDependsOnOutdatedVersion)
}

fn fuzz_binary_import(data: &[u8]) {
    let doc = seed_doc();
    let before = snapshot(&doc);

    if let Err(error) = doc.import(data) {
        if !is_prefix_import_error(&error) {
            assert_unchanged(&doc, &before);
        }
    }
}

fn fuzz_import_blob_meta(data: &[u8]) {
    let _ = LoroDoc::decode_import_blob_meta(data, false);
    let _ = LoroDoc::decode_import_blob_meta(data, true);
}

fn fuzz_json_import(data: &[u8]) {
    let Ok(json) = std::str::from_utf8(data) else {
        return;
    };

    let doc = seed_doc();
    let before = snapshot(&doc);

    if let Err(error) = doc.import_json_updates(json) {
        if !is_prefix_import_error(&error) {
            assert_unchanged(&doc, &before);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    fuzz_binary_import(data);
    fuzz_import_blob_meta(data);
    fuzz_json_import(data);
});
