use loro::{ExportMode, Index, LoroDoc, LoroMap, LoroMovableList, LoroResult, LoroText, ToJson};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

#[test]
fn movable_list_diff_apply_preserves_moves_container_replacements_and_reverse_patch(
) -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(91)?;

    let list = doc.get_movable_list("queue");
    list.push("alpha")?;
    let first_map = list.push_container(LoroMap::new())?;
    first_map.insert("id", "first")?;
    first_map.insert("rank", 1)?;
    let second_text = list.push_container(LoroText::new())?;
    second_text.insert(0, "second")?;
    list.push("tail")?;
    doc.commit();

    let v1 = doc.state_frontiers();
    let snapshot_v1 = doc.export(ExportMode::Snapshot)?;
    let value_v1 = deep_json(&doc);
    assert_eq!(
        value_v1,
        json!({
            "queue": ["alpha", {"id": "first", "rank": 1}, "second", "tail"]
        })
    );

    list.mov(2, 0)?;
    let replacement = list.insert_container(1, LoroMap::new())?;
    replacement.insert("id", "replacement")?;
    replacement.insert("rank", 2)?;
    list.delete(3, 1)?;
    list.set(3, "tail-updated")?;
    doc.commit();

    let v2 = doc.state_frontiers();
    let snapshot_v2 = doc.export(ExportMode::Snapshot)?;
    let value_v2 = deep_json(&doc);
    assert_eq!(
        value_v2,
        json!({
            "queue": ["second", {"id": "replacement", "rank": 2}, "alpha", "tail-updated"]
        })
    );

    let forward = doc.diff(&v1, &v2)?;
    let patched = LoroDoc::from_snapshot(&snapshot_v1)?;
    patched.apply_diff(forward)?;
    assert_eq!(deep_json(&patched), value_v2);

    let reverse = doc.diff(&v2, &v1)?;
    let restored = LoroDoc::from_snapshot(&snapshot_v2)?;
    restored.apply_diff(reverse)?;
    assert_eq!(deep_json(&restored), value_v1);

    Ok(())
}

#[test]
fn movable_list_creator_mover_and_editor_metadata_survives_remote_imports() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(201)?;
    let list = doc.get_movable_list("queue");
    list.push("draft")?;
    list.push("review")?;
    list.push("ship")?;
    let note = list.push_container(LoroText::new())?;
    note.insert(0, "note")?;
    doc.commit();

    let base_vv = doc.oplog_vv();
    for index in 0..list.len() {
        assert_eq!(list.get_creator_at(index), Some(201));
        assert_eq!(list.get_last_mover_at(index), Some(201));
        assert_eq!(list.get_last_editor_at(index), Some(201));
    }

    let remote = doc.fork();
    remote.set_peer_id(202)?;
    let remote_list = remote.get_movable_list("queue");
    remote_list.mov(2, 0)?;
    remote_list.set(1, "draft-updated")?;
    let replacement = remote_list.set_container(3, LoroMap::new())?;
    replacement.insert("kind", "replacement")?;
    remote.commit();

    let updates = remote.export(ExportMode::updates(&base_vv))?;
    doc.import(&updates)?;

    assert_eq!(
        list.get_deep_value().to_json_value(),
        json!(["ship", "draft-updated", "review", {"kind": "replacement"}])
    );
    assert_eq!(list.get_creator_at(0), Some(201));
    assert_eq!(list.get_last_mover_at(0), Some(202));
    assert_eq!(list.get_last_editor_at(0), Some(201));

    assert_eq!(list.get_creator_at(1), Some(201));
    assert_eq!(list.get_last_editor_at(1), Some(202));
    assert_eq!(list.get_creator_at(3), Some(201));
    assert_eq!(list.get_last_editor_at(3), Some(202));

    let restored = LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot)?)?;
    let restored_list = restored.get_movable_list("queue");
    assert_eq!(restored_list.get_creator_at(0), Some(201));
    assert_eq!(restored_list.get_last_mover_at(0), Some(202));
    assert_eq!(restored_list.get_last_editor_at(1), Some(202));
    assert_eq!(restored_list.get_last_editor_at(3), Some(202));
    assert_eq!(deep_json(&restored), deep_json(&doc));

    Ok(())
}

#[test]
fn movable_list_diff_apply_roundtrips_mixed_values_and_path_lookups() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(202)?;

    let root = doc.get_map("root");
    let queue = root.insert_container("queue", LoroMovableList::new())?;

    let first = queue.push_container(LoroMap::new())?;
    first.insert("name", "alpha")?;
    first.insert("bytes", vec![1_u8, 2, 3, 255])?;

    let second = queue.push_container(LoroText::new())?;
    second.insert(0, "beta")?;

    queue.push(true)?;
    doc.commit();

    let before = doc.state_frontiers();
    let before_snapshot = doc.export(ExportMode::Snapshot)?;
    let before_doc = LoroDoc::from_snapshot(&before_snapshot)?;

    assert_eq!(
        doc.get_by_str_path("root/queue/0/name")
            .expect("map child should resolve by path")
            .get_deep_value()
            .to_json_value(),
        json!("alpha")
    );
    assert_eq!(
        doc.get_by_path(&[
            Index::Key("root".into()),
            Index::Key("queue".into()),
            Index::Seq(1),
        ])
        .expect("text child should resolve by path")
        .get_deep_value()
        .to_json_value(),
        json!("beta")
    );
    assert_eq!(
        doc.get_by_str_path("root/queue/2")
            .expect("scalar value should resolve by path")
            .get_deep_value()
            .to_json_value(),
        json!(true)
    );

    queue.mov(1, 0)?;
    first.insert("rank", 1)?;
    queue.set(2, false)?;
    let third = queue.set_container(2, LoroMap::new())?;
    third.insert("kind", "replacement")?;
    doc.commit();

    let after = doc.state_frontiers();
    let after_snapshot = doc.export(ExportMode::Snapshot)?;

    assert_eq!(
        doc.get_by_str_path("root/queue/1/name")
            .expect("moved map child should still resolve")
            .get_deep_value()
            .to_json_value(),
        json!("alpha")
    );
    assert_eq!(
        doc.get_by_str_path("root/queue/0")
            .expect("moved text child should resolve")
            .get_deep_value()
            .to_json_value(),
        json!("beta")
    );
    assert_eq!(
        doc.get_by_str_path("root/queue/2/kind")
            .expect("replacement map should resolve")
            .get_deep_value()
            .to_json_value(),
        json!("replacement")
    );
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "root": {
                "queue": [
                    "beta",
                    {"name": "alpha", "bytes": [1, 2, 3, 255], "rank": 1},
                    {"kind": "replacement"}
                ]
            }
        })
    );

    let forward = doc.diff(&before, &after)?;
    let patched = LoroDoc::from_snapshot(&before_snapshot)?;
    patched.apply_diff(forward)?;
    assert_eq!(deep_json(&patched), deep_json(&doc));

    let reverse = doc.diff(&after, &before)?;
    let restored = LoroDoc::from_snapshot(&after_snapshot)?;
    restored.apply_diff(reverse)?;
    assert_eq!(deep_json(&restored), deep_json(&before_doc));

    assert_eq!(queue.get_creator_at(0), Some(202));
    assert_eq!(queue.get_last_mover_at(0), Some(202));
    assert_eq!(queue.get_last_editor_at(1), Some(202));

    Ok(())
}

#[test]
fn movable_list_pop_and_out_of_bounds_set_container_follow_contract() -> LoroResult<()> {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("queue");
    assert!(list.pop()?.is_none());

    list.push("a")?;
    list.push("b")?;
    assert_eq!(
        list.pop()?.unwrap().get_deep_value().to_json_value(),
        json!("b")
    );
    assert_eq!(
        list.pop()?.unwrap().get_deep_value().to_json_value(),
        json!("a")
    );
    assert!(list.pop()?.is_none());
    assert!(list.set_container(0, LoroMap::new()).is_err());

    let detached = loro::LoroMovableList::new();
    assert!(detached.pop()?.is_none());
    assert!(detached.set_container(0, LoroMap::new()).is_err());
    detached.push("x")?;
    let map = detached.set_container(0, LoroMap::new())?;
    map.insert("ok", true)?;
    assert_eq!(
        detached.get_deep_value().to_json_value(),
        json!([{"ok": true}])
    );

    Ok(())
}
