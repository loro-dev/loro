use crate::{
    loro::{AFTER_APPLY_DIFF_HANDLER, FAIL_APPLY_DIFF_AFTER_HANDLER},
    undo::UndoManager,
    LoroDoc,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[test]
fn failed_undo_and_redo_restore_text_history_and_events() {
    for mixed in [false, true] {
        let doc = LoroDoc::new_auto_commit();
        doc.set_peer_id(1).unwrap();
        let text = doc.get_text("text");
        text.insert(0, "a😀bc", crate::cursor::PosType::Unicode)
            .unwrap();
        doc.commit_then_renew();
        let undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);
        text.delete(1, 1, crate::cursor::PosType::Unicode).unwrap();
        text.mark(0, 2, "bold", true.into(), crate::cursor::PosType::Unicode)
            .unwrap();
        if mixed {
            doc.get_map("map").insert("changed", 42).unwrap();
            doc.get_list("list").push(42).unwrap();
            doc.get_movable_list("movable").push(42.into()).unwrap();
            doc.get_tree("tree")
                .create(crate::state::TreeParentId::Root)
                .unwrap();
            #[cfg(feature = "counter")]
            doc.get_counter("counter").increment(0.1).unwrap();
        }
        doc.commit_then_renew();
        let events = Arc::new(AtomicUsize::new(0));
        let captured = events.clone();
        let _sub = doc.subscribe_root(Arc::new(move |_| {
            captured.fetch_add(1, Ordering::Relaxed);
        }));

        for redo in [false, true] {
            let value = doc.get_deep_value();
            let delta = text.get_delta();
            let frontiers = doc.state_frontiers();
            let vv = doc.oplog_vv();
            let count = doc.oplog().lock().cached_visible_op_count();
            let stacks = (undo.undo_count(), undo.redo_count());
            for _ in 0..2 {
                let seen = events.load(Ordering::Relaxed);
                FAIL_APPLY_DIFF_AFTER_HANDLER.with(|flag| flag.set(true));
                let result = if redo { undo.redo() } else { undo.undo() };
                assert!(matches!(
                    result,
                    Err(crate::LoroError::OutOfBound {
                        pos: usize::MAX,
                        ..
                    })
                ));
                assert_eq!(doc.get_deep_value(), value);
                assert_eq!(text.get_delta(), delta);
                assert_eq!(doc.state_frontiers(), frontiers);
                assert_eq!(doc.oplog_vv(), vv);
                assert_eq!(doc.oplog().lock().cached_visible_op_count(), count);
                assert_eq!(doc.get_pending_txn_len(), 0);
                assert_eq!((undo.undo_count(), undo.redo_count()), stacks);
                assert_eq!(events.load(Ordering::Relaxed), seen);
            }
            assert!(if redo { undo.redo() } else { undo.undo() }.unwrap());
        }
        assert_eq!(text.get_delta(), {
            let restored = LoroDoc::new_auto_commit();
            restored
                .import(&doc.export(crate::encoding::ExportMode::Snapshot).unwrap())
                .unwrap();
            restored.get_text("text").get_delta()
        });
    }
}

#[test]
fn failed_undo_does_not_discard_a_concurrent_writers_edit() {
    use std::{sync::mpsc, thread, time::Duration};
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    text.insert(0, "abc", crate::cursor::PosType::Unicode)
        .unwrap();
    doc.commit_then_renew();
    let undo = UndoManager::new(&doc);
    text.delete(0, 1, crate::cursor::PosType::Unicode).unwrap();
    doc.commit_then_renew();
    let other = doc.clone();
    let (go, begin) = mpsc::channel();
    let (ready, attempted) = mpsc::channel();
    let (done, finished) = mpsc::channel();
    let writer = thread::spawn(move || {
        begin.recv().unwrap();
        ready.send(()).unwrap();
        other
            .get_text("text")
            .insert(2, "X", crate::cursor::PosType::Unicode)
            .unwrap();
        other.commit_then_renew();
        let _ = done.send(());
    });
    AFTER_APPLY_DIFF_HANDLER.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            go.send(()).unwrap();
            attempted.recv().unwrap();
            assert!(matches!(
                finished.recv_timeout(Duration::from_millis(50)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ));
        }));
    });
    FAIL_APPLY_DIFF_AFTER_HANDLER.with(|flag| flag.set(true));
    assert!(undo.undo().is_err());
    writer.join().unwrap();
    assert_eq!(text.to_string(), "bcX");
    assert_eq!(undo.undo_count(), 2);
    assert_eq!(undo.redo_count(), 0);
}
