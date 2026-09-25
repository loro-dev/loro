//! Containers deleted before a shallow root must not be carried in the exported
//! state, however many ops the snapshot retains after the root.

use loro::{ContainerID, ContainerTrait, ExportMode, Frontiers, LoroDoc, LoroMap};

/// `rows` child maps, all deleted, then the cut, then `edits_after_cut` unrelated ops.
fn doc_with_rows_deleted_before_cut(
    rows: usize,
    edits_after_cut: usize,
) -> (LoroDoc, Frontiers, Vec<ContainerID>) {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let map = doc.get_map("rows");
    let mut ids = Vec::new();
    for i in 0..rows {
        let row = map
            .insert_container(&format!("r{i}"), LoroMap::new())
            .unwrap();
        row.insert("title", format!("row {i}")).unwrap();
        ids.push(row.id());
    }
    doc.commit();
    for i in 0..rows {
        map.delete(&format!("r{i}")).unwrap();
    }
    doc.commit();
    let cut = doc.oplog_frontiers();
    let other = doc.get_map("other");
    for i in 0..edits_after_cut {
        other.insert(&format!("k{i}"), i as i64).unwrap();
        doc.commit();
    }
    (doc, cut, ids)
}

#[test]
fn shallow_snapshot_drops_containers_deleted_before_the_root() {
    // 300 retained ops is above the threshold that adds the latest-state overlay.
    for edits_after_cut in [0, 300] {
        let (doc, cut, ids) = doc_with_rows_deleted_before_cut(50, edits_after_cut);
        let bytes = doc.export(ExportMode::shallow_snapshot(&cut)).unwrap();
        let loaded = LoroDoc::new();
        loaded.import(&bytes).unwrap();
        assert_eq!(loaded.get_deep_value(), doc.get_deep_value());
        for id in &ids {
            assert!(
                !loaded.has_container(id),
                "{edits_after_cut} ops after the root: {id} was deleted before the root but is still stored"
            );
        }
    }
}

#[test]
fn state_only_snapshot_drops_containers_deleted_before_the_target() {
    let (doc, _, ids) = doc_with_rows_deleted_before_cut(50, 300);
    let bytes = doc
        .export(ExportMode::state_only(Some(&doc.oplog_frontiers())))
        .unwrap();
    let loaded = LoroDoc::new();
    loaded.import(&bytes).unwrap();
    assert_eq!(loaded.get_deep_value(), doc.get_deep_value());
    for id in &ids {
        assert!(
            !loaded.has_container(id),
            "{id} was deleted before the target but is still stored"
        );
    }
}

#[test]
fn shallow_snapshot_keeps_containers_created_after_the_root() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.get_map("seed").insert("a", 1).unwrap();
    doc.commit();
    let cut = doc.oplog_frontiers();
    let rows = doc.get_map("rows");
    let mut ids = Vec::new();
    for i in 0..300 {
        let row = rows
            .insert_container(&format!("r{i}"), LoroMap::new())
            .unwrap();
        row.insert("title", format!("row {i}")).unwrap();
        ids.push(row.id());
        doc.commit();
    }
    let bytes = doc.export(ExportMode::shallow_snapshot(&cut)).unwrap();
    let loaded = LoroDoc::new();
    loaded.import(&bytes).unwrap();
    assert_eq!(loaded.get_deep_value(), doc.get_deep_value());
    for id in &ids {
        assert!(
            loaded.has_container(id),
            "{id} was created after the root and must survive"
        );
    }
}
