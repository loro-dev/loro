//! Public undo identity contract from loro-dev/loro#1187.

use loro::{ContainerTrait, ExportMode, LoroDoc, LoroMap, ToJson, UndoManager};
use serde_json::json;

fn send(from: &LoroDoc, to: &LoroDoc) {
    to.import_with(&from.export(ExportMode::all_updates()).unwrap(), "import")
        .unwrap();
}

fn child(doc: &LoroDoc) -> LoroMap {
    doc.get_map("root")
        .get("child")
        .unwrap()
        .into_container()
        .unwrap()
        .into_map()
        .unwrap()
}

fn assert_delete_undo_identity(mergeable: bool, import_before_undo: bool) {
    let a = LoroDoc::new();
    a.set_peer_id(1).unwrap();
    let root = a.get_map("root");
    let original = if mergeable {
        root.ensure_mergeable_map("child").unwrap()
    } else {
        root.insert_container("child", LoroMap::new()).unwrap()
    };
    original.insert("base", 1).unwrap();
    a.commit();
    let original_id = original.id();

    let b = LoroDoc::new();
    b.set_peer_id(2).unwrap();
    send(&a, &b);
    let remote = child(&b);
    assert_eq!(remote.id(), original_id);

    let mut undo = UndoManager::new(&a);
    undo.set_merge_interval(0);
    undo.add_exclude_origin_prefix("import");
    root.delete("child").unwrap();
    a.commit();
    assert!(root.get("child").is_none());

    // B has seen neither the deletion nor its undo and edits the original child.
    remote.insert("remote", 2).unwrap();
    b.commit();
    if import_before_undo {
        send(&b, &a);
        assert!(root.get("child").is_none());
    }

    assert!(undo.undo().unwrap());
    let restored_id = child(&a).id();
    if mergeable {
        assert_eq!(restored_id, original_id);
    } else {
        assert_ne!(restored_id, original_id);
    }
    if !import_before_undo {
        send(&b, &a);
    }

    let expected_child = if mergeable {
        json!({"base": 1, "remote": 2})
    } else {
        json!({"base": 1})
    };
    assert_eq!(child(&a).get_deep_value().to_json_value(), expected_child);
    send(&a, &b);
    send(&b, &a);

    for doc in [&a, &b] {
        assert_eq!(child(doc).id(), restored_id);
        assert_eq!(
            doc.get_deep_value().to_json_value(),
            json!({"root": {"child": expected_child}})
        );
        // No remote operation is discarded or redirected: it still targets its cid.
        assert_eq!(
            doc.get_map(original_id.clone())
                .get_deep_value()
                .to_json_value(),
            json!({"base": 1, "remote": 2})
        );
    }
    assert_eq!(a.oplog_vv(), b.oplog_vv());
}

#[test]
fn regular_child_undo_reconstructs_before_remote_import() {
    assert_delete_undo_identity(false, false);
}

#[test]
fn regular_child_undo_reconstructs_after_remote_import() {
    assert_delete_undo_identity(false, true);
}

#[test]
fn mergeable_child_undo_reactivates_before_remote_import() {
    assert_delete_undo_identity(true, false);
}

#[test]
fn mergeable_child_undo_reactivates_after_remote_import() {
    assert_delete_undo_identity(true, true);
}

#[test]
fn undo_visibility_flag_keeps_regular_child_identity_and_concurrent_edits() {
    for import_before_undo in [true, false] {
        let a = LoroDoc::new();
        a.set_peer_id(1).unwrap();
        let original = a
            .get_map("root")
            .insert_container("child", LoroMap::new())
            .unwrap();
        original.insert("base", 1).unwrap();
        original.insert("deleted", false).unwrap();
        a.commit();
        let b = LoroDoc::new();
        b.set_peer_id(2).unwrap();
        send(&a, &b);

        let mut undo = UndoManager::new(&a);
        original.insert("deleted", true).unwrap();
        a.commit();
        child(&b).insert("remote", 2).unwrap();
        b.commit();
        if import_before_undo {
            send(&b, &a);
        }
        assert!(undo.undo().unwrap());
        send(&b, &a);
        send(&a, &b);

        for doc in [&a, &b] {
            assert_eq!(child(doc).id(), original.id());
            assert_eq!(
                doc.get_deep_value().to_json_value(),
                json!({"root": {"child": {"base": 1, "deleted": false, "remote": 2}}})
            );
        }
    }
}

#[test]
fn undo_regular_parent_deletion_also_reconstructs_mergeable_descendant() {
    for import_before_undo in [true, false] {
        let a = LoroDoc::new();
        a.set_peer_id(1).unwrap();
        let root = a.get_map("root");
        let parent = root.insert_container("child", LoroMap::new()).unwrap();
        let original = parent.ensure_mergeable_map("attrs").unwrap();
        original.insert("base", 1).unwrap();
        a.commit();
        let b = LoroDoc::new();
        b.set_peer_id(2).unwrap();
        send(&a, &b);

        let mut undo = UndoManager::new(&a);
        root.delete("child").unwrap();
        a.commit();
        b.get_map(original.id()).insert("remote", 2).unwrap();
        b.commit();
        if import_before_undo {
            send(&b, &a);
        }
        assert!(undo.undo().unwrap());
        send(&b, &a);
        send(&a, &b);

        for doc in [&a, &b] {
            let restored_parent = child(doc);
            assert_ne!(restored_parent.id(), parent.id());
            let restored = restored_parent
                .get("attrs")
                .unwrap()
                .into_container()
                .unwrap()
                .into_map()
                .unwrap();
            assert_ne!(restored.id(), original.id());
            assert!(restored.id().is_mergeable());
            assert_eq!(
                doc.get_deep_value().to_json_value(),
                json!({"root": {"child": {"attrs": {"base": 1}}}})
            );
            assert_eq!(
                doc.get_map(original.id()).get_deep_value().to_json_value(),
                json!({"base": 1, "remote": 2})
            );
        }
        assert_eq!(a.get_deep_value(), b.get_deep_value());
        assert_eq!(child(&a).id(), child(&b).id());
    }
}
