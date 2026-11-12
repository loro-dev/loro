use super::*;
use crate::{cursor::PosType, handler::HandlerTrait, loro::ExportMode};

#[test]
fn undo_split_large_cut_on_fork_restores_block_order() {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let original: String = (0..16000)
        .map(|i| (b'a' + (i % 26) as u8) as char)
        .collect();
    let text = doc.get_text("text");
    text.insert(0, &original, PosType::Unicode).unwrap();
    doc.commit_then_renew();
    let start = *doc.oplog_vv().get(&1).unwrap();
    for i in 0..1200 {
        let len = 1 + i % 9;
        text.delete(text.len_unicode() - len, len, PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();
    }
    let end = *doc.oplog_vv().get(&1).unwrap();
    let span = IdSpan::new(1, start, end);
    let fork = doc.fork();
    // Decoding creates DAG nodes at change-store block boundaries even though
    // every operation came from one peer and no remote change split the cut.
    assert!(fork.oplog.lock().split_span_based_on_deps(span).len() > 2);
    for target in [&doc, &fork] {
        drop(
            target
                .undo_internal(span, &mut Default::default(), None, &mut |_| {})
                .unwrap(),
        );
        assert_eq!(target.get_text("text").to_string(), original);
    }
}

#[test]
fn undo_split_direct_span_transforms_last_inverse_to_latest() {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    text.insert(0, "abcdef", PosType::Unicode).unwrap();
    doc.commit_then_renew();
    let start = *doc.oplog_vv().get(&1).unwrap();
    let other = LoroDoc::new_auto_commit();
    other.set_peer_id(2).unwrap();
    for i in 0..3 {
        text.delete(text.len_unicode() - 1, 1, PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();
        other
            .import(&doc.export(ExportMode::all_updates()).unwrap())
            .unwrap();
        // Include a remote edit after the final local span, too. This covers
        // Either::Left(latest), whereas UndoManager supplies Either::Right.
        other
            .get_text("text")
            .insert(0, "Z", PosType::Unicode)
            .unwrap();
        other.get_map("meta").insert("step", i).unwrap();
        other.commit_then_renew();
        doc.import(&other.export(ExportMode::all_updates()).unwrap())
            .unwrap();
    }
    let end = *doc.oplog_vv().get(&1).unwrap();
    let span = IdSpan::new(1, start, end);
    assert_eq!(doc.oplog.lock().split_span_based_on_deps(span).len(), 3);
    drop(
        doc.undo_internal(span, &mut Default::default(), None, &mut |_| {})
            .unwrap(),
    );
    assert_eq!(text.to_string(), "ZZZabcdef");
    assert_eq!(
        doc.get_map("meta").get_value(),
        other.get_map("meta").get_value()
    );
}
