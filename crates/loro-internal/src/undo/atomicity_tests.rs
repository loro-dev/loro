use super::*;
use crate::{
    cursor::PosType,
    encoding::ExportMode,
    handler::MovableListHandler,
    loro::{
        AFTER_APPLY_DIFF_HANDLER, FAIL_APPLY_DIFF_AFTER_HANDLER, FAIL_APPLY_DIFF_WITH_ARG_ERR,
        RUN_APPLY_DIFF_HOOK_AFTER_NATIVE_PLAN,
    },
};
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn application_errors_preserve_remote_row_metadata_and_both_identity_maps() {
    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    text.insert(0, "abc", PosType::Unicode).unwrap();
    let child = doc
        .get_map("holder")
        .insert_container("child", MovableListHandler::new_detached())
        .unwrap();
    child.push(7.into()).unwrap();
    let list = doc.get_movable_list("list");
    list.push(7.into()).unwrap();
    list.push(8.into()).unwrap();
    doc.commit_then_renew();
    let undo = UndoManager::new(&doc);
    undo.set_merge_interval(0);
    doc.get_map("older").insert("keep", 42).unwrap();
    doc.commit_then_renew();
    text.delete(0, 1, PosType::Unicode).unwrap();
    list.delete(0, 1).unwrap();
    doc.get_map("holder").delete("child").unwrap();
    doc.commit_then_renew();
    let remote = doc.fork();
    remote.set_peer_id(2).unwrap();
    remote
        .get_text("text")
        .insert(0, "R", PosType::Unicode)
        .unwrap();
    remote.commit_then_renew();
    doc.import(&remote.export(ExportMode::all_updates()).unwrap())
        .unwrap();

    let pops = Arc::new(AtomicUsize::new(0));
    let captured = pops.clone();
    undo.set_on_pop(Some(Box::new(move |_, _, _| {
        captured.fetch_add(1, Ordering::Relaxed);
    })));
    let events = Arc::new(AtomicUsize::new(0));
    let captured = events.clone();
    let _sub = doc.subscribe_root(Arc::new(move |_| {
        captured.fetch_add(1, Ordering::Relaxed);
    }));

    for redo in [false, true] {
        // A remote edit after the successful undo also gives the redo item a
        // nonempty transform base. Ordinary push would put it in a new row.
        if redo {
            remote
                .import(&doc.export(ExportMode::all_updates()).unwrap())
                .unwrap();
            remote
                .get_text("text")
                .insert(0, "S", PosType::Unicode)
                .unwrap();
            remote.commit_then_renew();
            doc.import(&remote.export(ExportMode::all_updates()).unwrap())
                .unwrap();
        }
        let mut meta = UndoItemMeta::new();
        meta.set_value("saved selection".into());
        if redo {
            undo.set_top_redo_meta(meta);
        } else {
            undo.set_top_undo_meta(meta);
        }
        let row = {
            let lock = undo.inner.lock();
            let inner = lock.borrow();
            let stack = if redo {
                &inner.redo_stack
            } else {
                &inner.undo_stack
            };
            stack.stack.back().unwrap().1.clone()
        };
        assert!(!row.lock().cid_to_events.is_empty());
        let history = format!("{:?}", undo.inner.lock().borrow());
        let containers = undo.container_remap.lock().clone();
        let elements = undo.element_remap.lock().clone();
        let value = doc.get_deep_value();
        let delta = text.get_delta();
        let frontiers = doc.state_frontiers();
        let vv = doc.oplog_vv();
        let seen = events.load(Ordering::Relaxed);
        let popped = pops.load(Ordering::Relaxed);
        for native in [false, true] {
            for arg_err in [false, true] {
                let current = list.clone();
                AFTER_APPLY_DIFF_HANDLER.with(|hook| {
                    *hook.borrow_mut() = Some(Box::new(move || {
                        // Native edits have really run before either failure.
                        assert_eq!(current.len(), if redo { 1 } else { 2 });
                    }));
                });
                RUN_APPLY_DIFF_HOOK_AFTER_NATIVE_PLAN.with(|flag| flag.set(native));
                FAIL_APPLY_DIFF_WITH_ARG_ERR.with(|flag| flag.set(arg_err));
                FAIL_APPLY_DIFF_AFTER_HANDLER.with(|flag| flag.set(true));
                let err = if redo { undo.redo() } else { undo.undo() }.unwrap_err();
                assert!(matches!(err, LoroError::ArgErr(_)) == arg_err);
                RUN_APPLY_DIFF_HOOK_AFTER_NATIVE_PLAN.with(|flag| assert!(!flag.get()));
                assert_eq!(doc.get_deep_value(), value);
                assert_eq!(text.get_delta(), delta);
                assert_eq!(doc.state_frontiers(), frontiers);
                assert_eq!(doc.oplog_vv(), vv);
                assert_eq!(doc.get_pending_txn_len(), 0);
                assert_eq!(format!("{:?}", undo.inner.lock().borrow()), history);
                assert_eq!(*undo.container_remap.lock(), containers);
                assert_eq!(*undo.element_remap.lock(), elements);
                let lock = undo.inner.lock();
                let inner = lock.borrow();
                let stack = if redo {
                    &inner.redo_stack
                } else {
                    &inner.undo_stack
                };
                assert!(Arc::ptr_eq(&stack.stack.back().unwrap().1, &row));
                assert_eq!(events.load(Ordering::Relaxed), seen);
                assert_eq!(pops.load(Ordering::Relaxed), popped);
            }
        }
        assert!(if redo { undo.redo() } else { undo.undo() }.unwrap());
        assert_eq!(pops.load(Ordering::Relaxed), popped + 1);
        assert_eq!(text.to_string(), if redo { "SRbc" } else { "aRbc" });
        assert_eq!(list.len(), if redo { 1 } else { 2 });
        assert_eq!(doc.get_map("holder").get("child").is_some(), !redo);
        assert_eq!(doc.get_map("older").get("keep"), Some(42.into()));
    }
}
