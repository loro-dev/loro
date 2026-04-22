use loro::{
    cursor::{PosType, Side},
    event::Diff,
    ContainerTrait, ExpandType, ExportMode, Index, LoroDoc, LoroList, LoroMap, LoroMovableList,
    LoroResult, LoroText, StyleConfig, StyleConfigMap, TextDelta, ToJson, TreeParentId,
    UndoManager, ValueOrContainer,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

fn assert_value(value: ValueOrContainer, expected: Value) {
    assert_eq!(value.get_deep_value().to_json_value(), expected);
}

fn sync_all(a: &LoroDoc, b: &LoroDoc) -> LoroResult<()> {
    let a_updates = a.export(ExportMode::updates(&b.oplog_vv()))?;
    let b_updates = b.export(ExportMode::updates(&a.oplog_vv()))?;
    a.import(&b_updates)?;
    b.import(&a_updates)?;
    Ok(())
}

#[test]
fn nested_state_roundtrips_through_snapshots_checkout_and_revert() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(10)?;

    let workspace = doc.get_map("workspace");
    workspace.insert("title", "Spec")?;

    let body = workspace.insert_container("body", LoroText::new())?;
    body.insert(0, "Hello world")?;
    body.mark(0..5, "bold", true)?;

    let tasks = workspace.insert_container("tasks", LoroList::new())?;
    let first_task = tasks.insert_container(0, LoroMap::new())?;
    first_task.insert("title", "draft")?;
    first_task.insert("done", false)?;
    tasks.insert(1, "loose note")?;

    let order = workspace.insert_container("order", LoroMovableList::new())?;
    order.push("todo")?;
    order.push("doing")?;
    order.push("done")?;
    order.mov(2, 1)?;

    let outline = doc.get_tree("outline");
    outline.enable_fractional_index(0);
    let root = outline.create(TreeParentId::Root)?;
    let child = outline.create_at(root, 0)?;
    outline.get_meta(root)?.insert("title", "Root")?;
    outline.get_meta(child)?.insert("title", "Child")?;

    doc.commit();
    let v1 = doc.state_frontiers();
    let expected_v1 = deep_json(&doc);

    assert_eq!(
        workspace.get("title").unwrap().get_deep_value(),
        "Spec".into()
    );
    assert_eq!(body.to_string(), "Hello world");
    assert_eq!(tasks.len(), 2);
    assert_eq!(
        order.get_deep_value().to_json_value(),
        json!(["todo", "done", "doing"])
    );
    assert_eq!(outline.children(TreeParentId::Root).unwrap(), vec![root]);
    assert_eq!(outline.children(root).unwrap(), vec![child]);
    assert_eq!(
        outline
            .get_meta(child)?
            .get("title")
            .unwrap()
            .get_deep_value()
            .to_json_value(),
        json!("Child")
    );

    let body_by_path = doc
        .get_by_path(&[Index::Key("workspace".into()), Index::Key("body".into())])
        .expect("body path should resolve");
    assert_value(body_by_path, json!("Hello world"));

    let path_to_body = doc
        .get_path_to_container(&body.id())
        .expect("attached nested text should have a path");
    assert_eq!(
        path_to_body.last().map(|(_, index)| index),
        Some(&Index::Key("body".into()))
    );

    let snapshot = doc.export(ExportMode::Snapshot)?;
    let restored = LoroDoc::from_snapshot(&snapshot)?;
    assert_eq!(deep_json(&restored), expected_v1);

    body.insert(body.len_unicode(), "\nSecond line")?;
    body.unmark(0..5, "bold")?;
    first_task.insert("done", true)?;
    tasks.delete(1, 1)?;
    order.set(0, "backlog")?;
    let second_child = outline.create_at(root, 1)?;
    outline
        .get_meta(second_child)?
        .insert("title", "Second child")?;
    doc.commit();
    let v2 = doc.state_frontiers();
    let expected_v2 = deep_json(&doc);

    assert_eq!(body.to_string(), "Hello world\nSecond line");
    assert_eq!(
        tasks.get_deep_value().to_json_value(),
        json!([{"done": true, "title": "draft"}])
    );
    assert_eq!(
        order.get_deep_value().to_json_value(),
        json!(["backlog", "done", "doing"])
    );
    assert_eq!(outline.children(root).unwrap(), vec![child, second_child]);

    doc.checkout(&v1)?;
    assert!(doc.is_detached());
    assert_eq!(deep_json(&doc), expected_v1);

    doc.checkout(&v2)?;
    assert_eq!(deep_json(&doc), expected_v2);
    doc.checkout_to_latest();

    doc.revert_to(&v1)?;
    assert_eq!(deep_json(&doc), expected_v1);

    let replica = LoroDoc::new();
    replica.import(&doc.export(ExportMode::all_updates())?)?;
    assert_eq!(deep_json(&replica), expected_v1);

    Ok(())
}

#[test]
fn concurrent_updates_converge_with_incremental_and_batch_imports() -> LoroResult<()> {
    let a = LoroDoc::new();
    a.set_peer_id(1)?;
    let b = LoroDoc::new();
    b.set_peer_id(2)?;

    let text = a.get_text("doc");
    text.insert(0, "base")?;
    let settings = a.get_map("settings");
    settings.insert("theme", "light")?;
    let tree = a.get_tree("outline");
    let root = tree.create(TreeParentId::Root)?;
    tree.get_meta(root)?.insert("title", "root")?;
    a.commit();

    let base = a.export(ExportMode::all_updates())?;
    b.import(&base)?;
    let base_vv = a.oplog_vv();

    a.get_text("doc").insert(4, " from A")?;
    a.get_list("a_items").push("a-only")?;
    a.get_tree("outline")
        .get_meta(root)?
        .insert("a_seen", true)?;
    a.commit();
    let a_update = a.export(ExportMode::updates(&base_vv))?;

    b.get_text("doc").insert(0, "B says ")?;
    b.get_map("settings").insert("font", "mono")?;
    b.get_movable_list("b_order").push("b-only")?;
    b.commit();
    let b_update = b.export(ExportMode::updates(&base_vv))?;

    a.import(&b_update)?;
    b.import(&a_update)?;
    assert_eq!(deep_json(&a), deep_json(&b));
    assert_eq!(a.get_text("doc").to_string(), "B says base from A");
    assert_eq!(
        a.get_map("settings").get_deep_value().to_json_value(),
        json!({"font": "mono", "theme": "light"})
    );
    assert_eq!(
        a.get_list("a_items").get_deep_value().to_json_value(),
        json!(["a-only"])
    );
    assert_eq!(
        a.get_movable_list("b_order")
            .get_deep_value()
            .to_json_value(),
        json!(["b-only"])
    );
    assert_eq!(
        a.get_tree("outline")
            .get_meta(root)?
            .get_deep_value()
            .to_json_value(),
        json!({"a_seen": true, "title": "root"})
    );

    let out_of_order = LoroDoc::new();
    let status = out_of_order.import_batch(&[a_update, b_update, base])?;
    assert!(status.pending.is_none());
    assert_eq!(deep_json(&out_of_order), deep_json(&a));

    Ok(())
}

#[test]
fn rich_text_unicode_styles_cursors_and_diffs_follow_contracts() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(7)?;

    let mut styles = StyleConfigMap::default_rich_text_config();
    styles.insert("link".into(), StyleConfig::new().expand(ExpandType::Before));
    styles.insert("tag".into(), StyleConfig::new().expand(ExpandType::None));
    doc.config_text_style(styles);

    let text = doc.get_text("text");
    text.insert(0, "A😀BC")?;
    text.mark(0..2, "link", "https://example.com")?;
    text.mark(2..4, "tag", "tail")?;

    let middle = text.get_cursor(1, Side::Middle).expect("cursor at emoji");
    let delta = text.slice_delta(1, 3, PosType::Unicode)?;
    assert_eq!(delta.len(), 2);
    match &delta[0] {
        TextDelta::Insert { insert, attributes } => {
            assert_eq!(insert, "😀");
            assert_eq!(
                attributes.as_ref().and_then(|x| x.get("link")),
                Some(&"https://example.com".into())
            );
        }
        other => panic!("expected emoji insert delta, got {other:?}"),
    }
    match &delta[1] {
        TextDelta::Insert { insert, attributes } => {
            assert_eq!(insert, "B");
            assert_eq!(
                attributes.as_ref().and_then(|x| x.get("tag")),
                Some(&"tail".into())
            );
        }
        other => panic!("expected B insert delta, got {other:?}"),
    }

    text.insert_utf16(3, "Z")?;
    assert_eq!(text.to_string(), "A😀ZBC");
    assert_eq!(
        doc.get_cursor_pos(&middle)
            .expect("cursor should still resolve after insertion")
            .current
            .pos,
        1
    );
    text.delete_utf16(3, 1)?;
    assert_eq!(text.to_string(), "A😀BC");

    doc.commit();
    let before_update = doc.state_frontiers();
    text.update_by_line("A😀BC\nnew line", Default::default())
        .expect("line update should finish");
    doc.commit();
    let after_update = doc.state_frontiers();

    let diff = doc.diff(&before_update, &after_update)?;
    assert!(diff.iter().any(|(_, diff)| matches!(diff, Diff::Text(_))));

    let patched = LoroDoc::new();
    patched.get_text("text").insert(0, "A😀BC")?;
    patched.commit();
    patched.apply_diff(diff)?;
    assert_eq!(patched.get_text("text").to_string(), "A😀BC\nnew line");

    let restored = LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot)?)?;
    assert_eq!(restored.get_text("text").to_string(), "A😀BC\nnew line");
    assert_eq!(deep_json(&restored), deep_json(&doc));

    Ok(())
}

#[test]
fn undo_redo_tracks_changes_without_undoing_remote_updates() -> LoroResult<()> {
    let local = LoroDoc::new();
    local.set_peer_id(31)?;
    let mut undo = UndoManager::new(&local);
    undo.set_merge_interval(0);

    let text = local.get_text("text");
    text.insert(0, "local")?;
    local.commit();
    undo.record_new_checkpoint()?;

    let remote = LoroDoc::new();
    remote.set_peer_id(32)?;
    remote.import(&local.export(ExportMode::all_updates())?)?;
    remote.get_text("text").insert(0, "remote ")?;
    remote.commit();

    local.import(&remote.export(ExportMode::updates(&local.oplog_vv()))?)?;
    assert_eq!(text.to_string(), "remote local");

    text.insert(text.len_unicode(), " edit")?;
    local.get_list("items").push("local item")?;
    local.commit();
    assert_eq!(text.to_string(), "remote local edit");
    assert_eq!(
        local.get_list("items").get_deep_value().to_json_value(),
        json!(["local item"])
    );

    assert!(undo.undo()?);
    assert_eq!(text.to_string(), "remote local");
    assert_eq!(
        local.get_list("items").get_deep_value().to_json_value(),
        json!([])
    );

    assert!(undo.redo()?);
    assert_eq!(text.to_string(), "remote local edit");
    assert_eq!(
        local.get_list("items").get_deep_value().to_json_value(),
        json!(["local item"])
    );

    sync_all(&local, &remote)?;
    assert_eq!(deep_json(&local), deep_json(&remote));

    Ok(())
}
