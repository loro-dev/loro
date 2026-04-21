use loro::{
    ContainerTrait, ExportMode, Index, LoroDoc, LoroMovableList, LoroResult, LoroText, LoroTree,
    ToJson, TreeParentId,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

fn assert_path_json(doc: &LoroDoc, path: &[Index], expected: Value) {
    let value = doc.get_by_path(path).expect("path should resolve");
    assert_eq!(value.get_deep_value().to_json_value(), expected);
}

#[test]
fn tree_nested_in_map_survives_path_snapshot_and_import() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(11)?;

    let workspace = doc.get_map("workspace");
    let outline = workspace.insert_container("outline", LoroTree::new())?;
    let agenda = workspace.insert_container("agenda", LoroMovableList::new())?;

    outline.enable_fractional_index(0);
    let root = outline.create(TreeParentId::Root)?;
    let child_a = outline.create_at(root, 0)?;
    let child_b = outline.create_at(root, 1)?;
    outline.get_meta(root)?.insert("title", "root")?;
    outline.get_meta(child_a)?.insert("title", "child-a")?;
    outline.get_meta(child_b)?.insert("title", "child-b")?;

    let history = outline
        .get_meta(root)?
        .insert_container("history", LoroMovableList::new())?;
    history.push("born")?;
    history.push("grow")?;
    history.mov(0, 1)?;

    agenda.push("draft")?;
    agenda.push("review")?;
    agenda.push("ship")?;
    doc.commit();

    let expected_v1 = deep_json(&doc);
    let snapshot_v1 = doc.export(ExportMode::Snapshot)?;

    assert_eq!(outline.children(TreeParentId::Root).unwrap(), vec![root]);
    assert_eq!(outline.children(root).unwrap(), vec![child_a, child_b]);
    assert_eq!(
        history.get_deep_value().to_json_value(),
        json!(["grow", "born"])
    );
    assert_path_json(
        &doc,
        &[
            Index::Key("workspace".into()),
            Index::Key("outline".into()),
            Index::Seq(0),
            Index::Key("history".into()),
            Index::Seq(0),
        ],
        json!("grow"),
    );
    assert_eq!(
        doc.get_by_str_path("workspace/outline/0/history/0")
            .expect("string path should resolve")
            .get_deep_value()
            .to_json_value(),
        json!("grow")
    );
    let history_path = doc
        .get_path_to_container(&history.id())
        .expect("nested history list should have a path");
    assert!(history_path
        .iter()
        .any(|(_, index)| matches!(index, Index::Key(key) if key.as_str() == "workspace")));
    assert!(history_path
        .iter()
        .any(|(_, index)| matches!(index, Index::Key(key) if key.as_str() == "history")));

    let restored = LoroDoc::from_snapshot(&snapshot_v1)?;
    assert_eq!(deep_json(&restored), expected_v1);
    assert_path_json(
        &restored,
        &[
            Index::Key("workspace".into()),
            Index::Key("outline".into()),
            Index::Seq(0),
            Index::Key("history".into()),
            Index::Seq(0),
        ],
        json!("grow"),
    );

    outline.mov_before(child_b, child_a)?;
    outline.delete(child_a)?;
    agenda.mov(2, 0)?;
    agenda.set(1, "in-review")?;
    doc.commit();

    let expected_v2 = deep_json(&doc);
    let snapshot_v2 = doc.export(ExportMode::Snapshot)?;

    let restored_v2 = LoroDoc::from_snapshot(&snapshot_v2)?;
    assert_eq!(deep_json(&restored_v2), expected_v2);
    assert_path_json(
        &restored_v2,
        &[
            Index::Key("workspace".into()),
            Index::Key("outline".into()),
            Index::Seq(0),
            Index::Key("history".into()),
            Index::Seq(0),
        ],
        json!("grow"),
    );

    assert_eq!(outline.children(TreeParentId::Root).unwrap(), vec![root]);
    assert_eq!(outline.children(root).unwrap(), vec![child_b]);
    assert!(outline.is_node_deleted(&child_a)?);
    assert_eq!(
        agenda.get_deep_value().to_json_value(),
        json!(["ship", "in-review", "review"])
    );

    Ok(())
}

#[test]
fn movable_list_converges_after_sync_and_roundtrips_through_paths() -> LoroResult<()> {
    let base = LoroDoc::new();
    base.set_peer_id(21)?;
    let board = base.get_map("board");
    let tasks = board.insert_container("tasks", LoroMovableList::new())?;
    tasks.insert(0, "draft")?;
    tasks.insert(1, "review")?;
    tasks.insert(2, "ship")?;
    let note = tasks.push_container(LoroText::new())?;
    note.insert(0, "notes")?;
    base.commit();

    let base_updates = base.export(ExportMode::all_updates())?;
    let base_vv = base.oplog_vv();

    let a = LoroDoc::new();
    a.set_peer_id(22)?;
    a.import(&base_updates)?;
    let b = LoroDoc::new();
    b.set_peer_id(23)?;
    b.import(&base_updates)?;

    let a_tasks = LoroMovableList::try_from_container(
        a.get_map("board")
            .get("tasks")
            .expect("tasks should exist")
            .into_container()
            .expect("tasks should be a container"),
    )
    .expect("tasks should be a movable list");
    a_tasks.mov(2, 0)?;
    a.get_map("board").insert("status", "ready")?;
    a.commit();
    let a_update = a.export(ExportMode::updates(&base_vv))?;

    let b_tasks = LoroMovableList::try_from_container(
        b.get_map("board")
            .get("tasks")
            .expect("tasks should exist")
            .into_container()
            .expect("tasks should be a container"),
    )
    .expect("tasks should be a movable list");
    b_tasks.push("deploy")?;
    b.get_map("board").insert("priority", "p1")?;
    b.commit();
    let b_update = b.export(ExportMode::updates(&base_vv))?;

    a.import(&b_update)?;
    b.import(&a_update)?;
    assert_eq!(deep_json(&a), deep_json(&b));
    assert_eq!(
        deep_json(&a),
        json!({
            "board": {
                "priority": "p1",
                "status": "ready",
                "tasks": ["ship", "draft", "review", "notes", "deploy"]
            }
        })
    );

    let batch = LoroDoc::new();
    batch.set_peer_id(24)?;
    let status = batch.import_batch(&[a_update, b_update, base_updates])?;
    assert!(status.pending.is_none());
    assert_eq!(deep_json(&batch), deep_json(&a));
    assert_eq!(
        batch
            .get_by_str_path("board/tasks/0")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("ship")
    );
    assert_eq!(
        batch
            .get_by_str_path("board/tasks/3")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("notes")
    );
    assert_eq!(
        batch
            .get_by_str_path("board/tasks/4")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("deploy")
    );

    Ok(())
}
