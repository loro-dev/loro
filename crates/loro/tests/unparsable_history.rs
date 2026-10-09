//! Unreadable lazy history must come back as `DecodeError` from the public
//! reads that used to unwrap it: version, JSON export, cursors, `apply_diff`
//! rollback, the next shallow import, and `fork_at`.

use loro::event::{Diff, DiffBatch};
use loro::{
    ContainerID, ContainerType, ExportMode, Frontiers, LoroDoc, LoroError, TextDelta,
    VersionVector, ID,
};

const FIXTURE: &[u8] =
    include_bytes!("../../loro-wasm/tests/fixtures/unparsable-history.snapshot.bin");

fn assert_decode(err: &LoroError) {
    assert!(
        matches!(err, LoroError::DecodeError(_)),
        "expected DecodeError, got {err:?}"
    );
}

fn snapshot_with_truncated_peer1_block(snapshot: &[u8]) -> (Vec<u8>, ID) {
    use loro_kv_store::{mem_store::MemKvConfig, MemKvStore};
    use std::ops::Bound;
    let mut at = 22;
    let mut sections = Vec::new();
    for _ in 0..3 {
        let len = u32::from_le_bytes(snapshot[at..at + 4].try_into().unwrap()) as usize;
        at += 4;
        sections.push(snapshot[at..at + len].to_vec());
        at += len;
    }
    let mut kv = MemKvStore::new(MemKvConfig::new());
    kv.import_all(sections[0].clone().into()).unwrap();
    let blocks: Vec<_> = kv
        .scan(Bound::Unbounded, Bound::Unbounded)
        .filter(|(key, _)| key.len() == 12 && ID::from_bytes(key).peer == 1)
        .collect();
    assert!(
        blocks.len() >= 3,
        "need a non-frontier peer-1 block, found {}",
        blocks.len()
    );
    let (key, bytes) = &blocks[0];
    let id = ID::from_bytes(key).inc(1);
    kv.set(key, bytes.slice(..bytes.len() / 2));
    sections[0] = kv.export_all().to_vec();
    let mut forged = snapshot[..22].to_vec();
    for section in sections {
        forged.extend_from_slice(&(section.len() as u32).to_le_bytes());
        forged.extend_from_slice(&section);
    }
    let checksum = xxhash_rust::xxh32::xxh32(&forged[20..], u32::from_le_bytes(*b"LORO"));
    forged[16..20].copy_from_slice(&checksum.to_le_bytes());
    (forged, id)
}

#[test]
fn state_vv_of_the_fixture_uses_the_cached_oplog_vv() {
    let doc = LoroDoc::new();
    doc.import(FIXTURE).unwrap();
    assert_eq!(doc.state_frontiers(), doc.oplog_frontiers());
    let vv = doc.state_vv().unwrap();
    assert_eq!(vv, doc.oplog_vv());
    assert_eq!(doc.try_state_vv().unwrap(), doc.oplog_vv());

    // The tip fast path does not walk this non-frontier id. The block that
    // contains `1@1` is truncated, so the walk must fail closed.
    let err = doc
        .try_frontiers_to_vv(&Frontiers::from(ID::new(1, 1)))
        .unwrap_err();
    assert_decode(&err);
    let err = doc.checkout(&Frontiers::from(ID::new(1, 1))).unwrap_err();
    assert_decode(&err);
    assert_eq!(doc.state_frontiers(), doc.oplog_frontiers());
}

#[test]
fn json_export_of_a_truncated_peer_returns_decode_error() {
    let doc = LoroDoc::new();
    doc.import(FIXTURE).unwrap();
    let mut start = VersionVector::default();
    start.set_end(ID::new(1, 2));
    start.set_end(ID::new(2, 1));
    let err = doc
        .try_export_json_updates(&start, &doc.oplog_vv())
        .unwrap_err();
    assert_decode(&err);
    let err = doc
        .export_json_updates(&start, &doc.oplog_vv())
        .unwrap_err();
    assert_decode(&err);
}

#[test]
fn apply_diff_does_not_panic_or_advance_the_doc_when_history_is_unreadable() {
    let doc = LoroDoc::new();
    doc.import(FIXTURE).unwrap();
    let frontiers = doc.state_frontiers();
    let value = doc.get_deep_value();
    let mut batch = DiffBatch::default();
    batch
        .push(
            ContainerID::new_root("text", ContainerType::Text),
            Diff::Text(vec![TextDelta::Insert {
                insert: "x".into(),
                attributes: None,
            }]),
        )
        .unwrap();
    batch
        .push(
            ContainerID::new_normal(ID::new(99, 0), ContainerType::Text),
            Diff::Text(vec![]),
        )
        .unwrap();
    let err = doc.apply_diff(batch).unwrap_err();
    assert!(err.to_string().contains("Decode error"), "{err:?}");
    assert_eq!(doc.state_frontiers(), frontiers);
    assert_eq!(doc.oplog_frontiers(), frontiers);
    assert_eq!(doc.get_deep_value(), value);
    doc.get_text("text").insert(0, "still works").unwrap();
}

#[test]
fn deleted_cursor_reports_unreadable_history() {
    let source = LoroDoc::new();
    source.set_peer_id(1).unwrap();
    source.set_change_merge_interval(-1);
    let text = source.get_text("text");
    text.insert(0, "cursor target").unwrap();
    source.commit();
    let cursor = text.get_cursor(0, Default::default()).unwrap();
    text.delete(0, text.len_unicode()).unwrap();
    source.commit();
    for i in 0..600 {
        text.insert(0, &format!("line-{i:04}-xxxxxxxxxxxxxxxxxxxxxxxx"))
            .unwrap();
        source.get_map("map").insert("key", i).unwrap();
        source.commit();
    }
    let snapshot = source.export(ExportMode::Snapshot).unwrap();
    let (forged, _) = snapshot_with_truncated_peer1_block(&snapshot);
    let doc = LoroDoc::new();
    doc.import(&forged).unwrap();
    let err = doc.get_cursor_pos(&cursor).unwrap_err();
    assert!(
        matches!(err, loro::CannotFindRelativePosition::HistoryUnreadable),
        "{err:?}"
    );
    let err = doc.try_get_cursor_pos(&cursor).unwrap_err();
    assert_decode(&err);
}

#[test]
fn shallow_import_of_a_later_update_returns_decode_error_without_advancing() {
    let root = LoroDoc::new();
    root.set_peer_id(2).unwrap();
    root.get_map("root").insert("key", 0).unwrap();
    root.commit();
    let shallow_root = root.oplog_frontiers();
    let source = LoroDoc::new();
    source.set_peer_id(1).unwrap();
    source.set_change_merge_interval(-1);
    source
        .import(&root.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    for i in 0..600 {
        source
            .get_text("text")
            .insert(0, &format!("line-{i:04}-xxxxxxxxxxxxxxxxxxxxxxxx"))
            .unwrap();
        source.get_map("map").insert("key", i).unwrap();
        source.commit();
    }
    let shallow = source
        .export(ExportMode::shallow_snapshot(&shallow_root))
        .unwrap();
    let (forged, id) = snapshot_with_truncated_peer1_block(&shallow);
    let remote = source.fork_at(&Frontiers::from(id)).unwrap();
    remote.set_peer_id(9).unwrap();
    remote.get_map("incoming").insert("key", 1).unwrap();
    let update = remote
        .export(ExportMode::updates(&source.oplog_vv()))
        .unwrap();

    let doc = LoroDoc::new();
    doc.import(&forged).unwrap();
    assert!(doc.is_shallow());
    let frontiers = doc.state_frontiers();
    let value = doc.get_deep_value();
    assert_eq!(doc.oplog_frontiers(), frontiers);
    let err = doc.import(&update).unwrap_err();
    assert_decode(&err);
    assert_eq!(doc.state_frontiers(), frontiers);
    assert_eq!(doc.oplog_frontiers(), frontiers);
    assert_eq!(doc.get_deep_value(), value);
}

#[test]
fn fork_at_returns_decode_error_not_unknown() {
    let doc = LoroDoc::new();
    doc.import(FIXTURE).unwrap();
    let err = doc.fork_at(&Frontiers::from(ID::new(1, 1))).unwrap_err();
    assert_decode(&err);
    assert_eq!(doc.oplog_frontiers(), doc.state_frontiers());
}
