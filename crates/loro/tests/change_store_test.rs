use loro::{ExportMode, Frontiers, LoroDoc, LoroMap, ID};

/// Regression test for a checkout hang after snapshot import.
///
/// A single commit whose ops span more than `MAX_BLOCK_SIZE * 8` lamports is
/// split into multiple change-store blocks on export. Blocks decoded from the
/// snapshot used to record a degenerate `lamport_range` (its end was the start
/// lamport of the block's last change), which sent the lamport binary search
/// in `ChangeStore::get_change_by_lamport_lte` into an infinite loop when the
/// movable-list diff calculator resolved historical positions during checkout.
#[test]
fn checkout_after_importing_block_split_change() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let list = doc.get_movable_list("list");
    for i in 0..512 {
        doc.get_text(format!("t{i}"))
            .insert(0, &"x".repeat(100))
            .unwrap();
        list.insert(list.len(), "ref").unwrap();
    }
    doc.commit();
    let snapshot = doc.export(ExportMode::Snapshot).unwrap();

    let doc2 = LoroDoc::new();
    doc2.import(&snapshot).unwrap();
    doc2.checkout(&Frontiers::from_id(ID::new(1, 1))).unwrap();
    assert_eq!(doc2.get_text("t0").to_string(), "xx");
    assert_eq!(doc2.get_movable_list("list").len(), 0);
}

#[test]
fn test_compact_change_store() {
    let doc = LoroDoc::new();
    doc.set_peer_id(0).unwrap();
    let text = doc.get_text("text");
    for i in 0..100 {
        text.insert(i, "hello").unwrap();
    }

    let list = doc.get_list("list");
    for _ in 0..100 {
        let map = list.push_container(LoroMap::new()).unwrap();
        for j in 0..100 {
            map.insert(&j.to_string(), j).unwrap();
        }
    }

    doc.commit();
    doc.compact_change_store();
    doc.checkout(&ID::new(0, 60).into()).unwrap();
}
