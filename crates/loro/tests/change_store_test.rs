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

/// `get_change_with_lamport_lte` (`getChangeAtLamport` in JS) read the block range
/// of KV-only blocks one field off, so after loading a snapshot most lookups
/// returned `None` or the wrong change.
#[test]
fn get_change_with_lamport_lte_after_snapshot_import() {
    let src = LoroDoc::new();
    src.set_peer_id(7).unwrap();
    let text = src.get_text("t");
    for i in 0..300 {
        text.insert(0, &format!("{i}-abcdefghijklmnopqrstuvwxyz"))
            .unwrap();
        src.commit();
    }
    let end = src.oplog_vv().get(&7).copied().unwrap() as u32;
    for lamport in (0..end).step_by(97) {
        let expected = src.with_oplog(|o| {
            o.get_change_with_lamport_lte(7, lamport)
                .map(|c| (c.id(), c.lamport()))
        });
        // Fresh load for every lookup, so the answer comes from the KV blocks.
        let fresh = LoroDoc::new();
        fresh
            .import(&src.export(ExportMode::Snapshot).unwrap())
            .unwrap();
        let got = fresh.with_oplog(|o| {
            o.get_change_with_lamport_lte(7, lamport)
                .map(|c| (c.id(), c.lamport()))
        });
        assert_eq!(got, expected, "lamport {lamport}");
        assert!(expected.is_some());
    }
}
