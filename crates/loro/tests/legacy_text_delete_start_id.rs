//! Deletes written by WASM `loro-crdt` <= 1.16.3 can record a `start_id` that names
//! the wrong characters around astral characters (loro-dev/loro#1149). The ops
//! can't be repaired; this pins the documented migration advice
//! (`context/richtext-insert-positions.md`): full imports and shallow snapshots
//! rooted after the bad delete give the right text.

use loro::{ExportMode, JsonOpContent, JsonTextOp, LoroDoc, ID};

/// The issue's edits, with the delete rewritten into the single op an affected
/// build wrote: `{pos: 0, len: 3, start_id: 0@7}` names "a😀b" (IDs 0..=2), but
/// "a😀x" (IDs 0, 1, 3) was deleted.
fn legacy_history() -> LoroDoc {
    let writer = LoroDoc::new();
    writer.set_peer_id(7).unwrap();
    let text = writer.get_text("t");
    text.insert(0, "a😀b").unwrap();
    writer.commit();
    text.insert(2, "x").unwrap(); // "a😀xb"
    writer.commit();
    text.delete(0, 3).unwrap(); // deletes "a😀x"
    writer.commit();
    text.insert(1, "!").unwrap();
    writer.commit();

    let mut json = writer.export_json_updates(&Default::default(), &writer.oplog_vv());
    for change in json.changes.iter_mut() {
        let deletes: Vec<usize> = change
            .ops
            .iter()
            .enumerate()
            .filter(|(_, op)| matches!(op.content, JsonOpContent::Text(JsonTextOp::Delete { .. })))
            .map(|(i, _)| i)
            .collect();
        if deletes.is_empty() {
            continue;
        }
        let counter = deletes
            .iter()
            .map(|&i| change.ops[i].counter)
            .min()
            .unwrap();
        let mut op = change.ops[deletes[0]].clone();
        op.counter = counter;
        op.content = JsonOpContent::Text(JsonTextOp::Delete {
            pos: 0,
            len: 3,
            start_id: ID::new(0, 0), // peer index 0 is peer 7
        });
        // The deletes are adjacent and cover the same counters as the one op.
        for &i in deletes[1..].iter().rev() {
            change.ops.remove(i);
        }
        change.ops[deletes[0]] = op;
    }
    let doc = LoroDoc::new();
    doc.import_json_updates(json).unwrap();
    doc
}

#[test]
fn full_import_and_shallow_snapshot_after_the_delete_are_right() {
    let full = legacy_history();
    assert_eq!(full.get_text("t").to_string(), "b!");

    let copy = LoroDoc::new();
    copy.import(&full.export(ExportMode::Snapshot).unwrap())
        .unwrap();
    assert_eq!(copy.get_text("t").to_string(), "b!");

    let shallow = LoroDoc::new();
    shallow
        .import(
            &full
                .export(ExportMode::shallow_snapshot(&full.oplog_frontiers()))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(shallow.get_text("t").to_string(), "b!");
}

/// The forged history does reproduce loro-dev/loro#1149: a shallow snapshot rooted
/// before the delete labels the deleted placeholders with the bad `start_id` and
/// ends with other text. This documents the open part of the issue; update it if
/// the shallow replay stops trusting `start_id`.
#[test]
fn shallow_snapshot_rooted_before_the_delete_still_diverges() {
    let full = legacy_history();
    let before_delete = full.vv_to_frontiers(&{
        let mut vv = loro::VersionVector::new();
        vv.insert(7, 5); // "a😀b" and "x", before the delete
        vv
    });
    let shallow = LoroDoc::new();
    shallow
        .import(
            &full
                .export(ExportMode::shallow_snapshot(&before_delete))
                .unwrap(),
        )
        .unwrap();
    assert_eq!(shallow.get_text("t").to_string(), "x!");
}
