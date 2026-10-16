//! Re-parsing a change block must not copy its text or list values into a
//! doc-wide store again (zeronsh/zeron#905). See `context/text-arena-growth.md`.

use crate::{
    cursor::PosType, loro::ExportMode, op::InnerContent, version::Frontiers, LoroDoc, LoroValue,
    ToJson,
};

#[test]
fn exporting_while_streaming_text_does_not_grow_the_string_arena() {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    let token = "token ";
    let ticks = 2000;
    for _ in 0..ticks {
        text.insert(text.len_unicode(), token, PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();
        // Turns the block back into bytes; the next commit parses it again.
        doc.export(ExportMode::Snapshot).unwrap();
    }

    let written = token.len() * ticks;
    let arena = doc.oplog().lock().arena.str_len_bytes();
    // Before the fix the arena held every re-parse of the block: ~1000x here.
    assert!(
        arena <= 2 * written,
        "string arena holds {arena} bytes for {written} bytes of text"
    );
    assert_eq!(text.to_string(), token.repeat(ticks));
}

/// Tracker diffs take text from the ops, which now live in block-local
/// buffers: check concurrent, non-ASCII and long inserts across re-parsed
/// blocks against a fresh replay at every version.
#[test]
fn checkout_text_from_reparsed_blocks_matches_replay() {
    let a = LoroDoc::new_auto_commit();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new_auto_commit();
    b.set_peer_id(2).unwrap();
    let long = "长文本😀".repeat(120);
    let mut versions: Vec<Frontiers> = Vec::new();
    for i in 0..40 {
        let (doc, s) = if i % 3 == 0 {
            (&b, long.as_str())
        } else {
            (&a, if i % 2 == 0 { "ab" } else { "文字😀" })
        };
        let text = doc.get_text("text");
        let pos = (i * 7) % (text.len_unicode() + 1);
        text.insert(pos, s, PosType::Unicode).unwrap();
        if i % 5 == 4 {
            let len = text.len_unicode();
            text.delete(len / 3, (len / 4).min(9), PosType::Unicode)
                .unwrap();
        }
        doc.commit_then_renew();
        doc.export(ExportMode::Snapshot).unwrap();
        if i % 4 == 3 {
            a.import(&b.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            b.import(&a.export(ExportMode::all_updates()).unwrap())
                .unwrap();
        }
        versions.push(doc.oplog_frontiers());
    }
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let snapshot = a.export(ExportMode::Snapshot).unwrap();
    for f in versions.iter().rev().chain(versions.iter()) {
        a.checkout(f).unwrap();
        let expected = LoroDoc::new();
        expected.import(&snapshot).unwrap();
        expected.checkout(f).unwrap();
        assert_eq!(
            a.get_text("text").to_string(),
            expected.get_text("text").to_string()
        );
        a.export(ExportMode::Snapshot).unwrap();
    }
    a.checkout_to_latest();
    let replay = LoroDoc::new();
    replay
        .import(&a.export(ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(
        a.get_text("text").to_string(),
        replay.get_text("text").to_string()
    );
}

/// List values are owned by their ops. Streaming pushes with an export after
/// each commit keeps one copy per value, and a local push still merges into the
/// re-parsed op that precedes it.
#[test]
fn exporting_while_pushing_list_values_keeps_one_copy() {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let list = doc.get_list("list");
    let ticks = 500;
    for i in 0..ticks {
        list.push(i as i64).unwrap();
        doc.commit_then_renew();
        doc.export(ExportMode::Snapshot).unwrap();
    }

    let mut stored = 0;
    let mut ops = 0;
    doc.oplog()
        .lock()
        .change_store()
        .visit_all_changes(&mut |change| {
            for op in change.ops().iter() {
                if let InnerContent::List(crate::container::list::list_op::InnerListOp::Insert {
                    slice,
                    ..
                }) = &op.content
                {
                    stored += slice.len();
                    ops += 1;
                }
            }
        });
    assert_eq!(stored, ticks);
    assert!(
        ops < ticks / 4,
        "{ops} insert ops for {ticks} consecutive pushes"
    );
    let expected: Vec<LoroValue> = (0..ticks as i64).map(LoroValue::from).collect();
    assert_eq!(
        doc.get_deep_value().to_json_value()["list"],
        LoroValue::from(expected).to_json_value()
    );
}

/// Checkouts of concurrent list and movable-list inserts from re-parsed
/// blocks match a fresh replay.
#[test]
fn checkout_list_values_from_reparsed_blocks_matches_replay() {
    let a = LoroDoc::new_auto_commit();
    a.set_peer_id(1).unwrap();
    let b = LoroDoc::new_auto_commit();
    b.set_peer_id(2).unwrap();
    let mut versions: Vec<Frontiers> = Vec::new();
    for i in 0..60i64 {
        let doc = if i % 3 == 0 { &b } else { &a };
        let list = doc.get_list("list");
        let movable = doc.get_movable_list("movable");
        let pos = (i as usize * 7) % (list.len() + 1);
        list.insert(pos, i).unwrap();
        list.insert(pos, format!("v{i}")).unwrap();
        let mpos = (i as usize * 5) % (movable.len() + 1);
        movable.insert(mpos, i).unwrap();
        if i % 5 == 4 {
            list.delete(list.len() / 3, 2.min(list.len() / 3)).unwrap();
            if movable.len() > 2 {
                movable.mov(0, movable.len() - 1).unwrap();
                movable.set(1, i * 100).unwrap();
            }
        }
        doc.commit_then_renew();
        doc.export(ExportMode::Snapshot).unwrap();
        if i % 4 == 3 {
            a.import(&b.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            b.import(&a.export(ExportMode::all_updates()).unwrap())
                .unwrap();
        }
        versions.push(doc.oplog_frontiers());
    }
    a.import(&b.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let snapshot = a.export(ExportMode::Snapshot).unwrap();
    for f in versions.iter().rev().chain(versions.iter()) {
        a.checkout(f).unwrap();
        let expected = LoroDoc::new();
        expected.import(&snapshot).unwrap();
        expected.checkout(f).unwrap();
        assert_eq!(
            a.get_deep_value().to_json_value(),
            expected.get_deep_value().to_json_value()
        );
        a.export(ExportMode::Snapshot).unwrap();
    }
}
