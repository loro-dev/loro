use loro::{
    cursor::{PosType, Side},
    event::Diff,
    ExpandType, ExportMode, LoroDoc, LoroResult, LoroValue, StyleConfig, StyleConfigMap, TextDelta,
    ToJson, UndoItemMeta, UndoManager, UndoOrRedo,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

fn collect_insert_text(delta: &[TextDelta]) -> String {
    delta
        .iter()
        .map(|segment| match segment {
            TextDelta::Insert { insert, .. } => insert.as_str(),
            other => panic!("expected insert segment, got {other:?}"),
        })
        .collect()
}

fn mark_segment<'a>(delta: &'a [TextDelta], needle: char) -> &'a TextDelta {
    delta
        .iter()
        .find(|segment| matches!(segment, TextDelta::Insert { insert, .. } if insert.contains(needle)))
        .expect("needle should be present in delta")
}

#[test]
fn mark_expand_policies_follow_boundary_rules() -> LoroResult<()> {
    {
        let doc = LoroDoc::new();
        let mut styles = StyleConfigMap::default_rich_text_config();
        styles.insert("after".into(), StyleConfig::new().expand(ExpandType::After));
        doc.config_text_style(styles);

        let text = doc.get_text("text");
        text.insert(0, "AB")?;
        text.mark(0..2, "after", true)?;
        doc.commit();
        text.insert(2, "x")?;

        let delta = text.slice_delta(0, text.len_unicode(), PosType::Unicode)?;
        assert_eq!(collect_insert_text(&delta), "ABx");
        match mark_segment(&delta, 'x') {
            TextDelta::Insert { attributes, .. } => {
                assert_eq!(
                    attributes.as_ref().and_then(|attrs| attrs.get("after")),
                    Some(&true.into())
                );
            }
            _ => unreachable!(),
        }
    }

    {
        let doc = LoroDoc::new();
        let mut styles = StyleConfigMap::default_rich_text_config();
        styles.insert("none".into(), StyleConfig::new().expand(ExpandType::None));
        doc.config_text_style(styles);

        let text = doc.get_text("text");
        text.insert(0, "AB")?;
        text.mark(0..2, "none", true)?;
        doc.commit();
        text.insert(0, "x")?;

        let delta = text.slice_delta(0, text.len_unicode(), PosType::Unicode)?;
        assert_eq!(collect_insert_text(&delta), "xAB");
        match mark_segment(&delta, 'x') {
            TextDelta::Insert { attributes, .. } => {
                assert!(attributes
                    .as_ref()
                    .and_then(|attrs| attrs.get("none"))
                    .is_none());
            }
            _ => unreachable!(),
        }
    }

    Ok(())
}

#[test]
fn update_by_line_diff_snapshot_and_import_stay_consistent() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(7)?;
    let text = doc.get_text("text");
    text.insert(0, "A😀BC\nsecond line\nthird line")?;
    let cursor = text
        .get_cursor(1, Side::Middle)
        .expect("cursor should resolve");

    let before_frontiers = doc.state_frontiers();
    let before_snapshot = doc.export(ExportMode::Snapshot)?;

    text.update_by_line(
        "A😀BC\nsecond line updated\nthird line\nfourth",
        Default::default(),
    )
    .expect("line update should succeed");
    doc.commit();
    let after_frontiers = doc.state_frontiers();

    assert_eq!(
        text.to_string(),
        "A😀BC\nsecond line updated\nthird line\nfourth"
    );
    let cursor_pos = doc.get_cursor_pos(&cursor).expect("cursor should resolve");
    assert!(cursor_pos.update.is_none());
    assert_eq!(cursor_pos.current.pos, 1);

    let diff = doc.diff(&before_frontiers, &after_frontiers)?;
    assert!(diff.iter().any(|(_, diff)| matches!(diff, Diff::Text(_))));

    let patched = LoroDoc::from_snapshot(&before_snapshot)?;
    patched.apply_diff(diff)?;
    assert_eq!(patched.get_text("text").to_string(), text.to_string());

    let imported = LoroDoc::new();
    imported.import(&doc.export(ExportMode::all_updates())?)?;
    assert_eq!(deep_json(&imported), deep_json(&doc));

    let restored = LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot)?)?;
    assert_eq!(restored.get_text("text").to_string(), text.to_string());
    assert_eq!(deep_json(&restored), deep_json(&doc));

    Ok(())
}

#[test]
fn grouped_undo_survives_remote_imports_and_tracks_stack_state() -> LoroResult<()> {
    let local = LoroDoc::new();
    local.set_peer_id(31)?;
    let mut undo = UndoManager::new(&local);
    undo.set_merge_interval(0);

    let text = local.get_text("text");
    text.insert(0, "base")?;
    local.commit();
    undo.record_new_checkpoint()?;

    undo.group_start()?;
    local.get_list("items").push("task")?;
    local.get_map("meta").insert("tag", "local")?;
    undo.group_end();
    local.commit();
    undo.record_new_checkpoint()?;

    let remote = LoroDoc::new();
    remote.set_peer_id(32)?;
    remote.import(&local.export(ExportMode::all_updates())?)?;
    remote.get_text("text").insert(0, "remote ")?;
    remote.commit();

    local.import(&remote.export(ExportMode::updates(&local.oplog_vv()))?)?;
    assert_eq!(text.to_string(), "remote base");
    assert_eq!(
        local.get_list("items").get_deep_value().to_json_value(),
        json!(["task"])
    );
    assert_eq!(
        local.get_map("meta").get_deep_value().to_json_value(),
        json!({"tag": "local"})
    );
    assert!(undo.can_undo());
    assert!(!undo.can_redo());
    assert!(undo.undo_count() > 0);
    assert_eq!(undo.redo_count(), 0);

    assert!(undo.undo()?);
    assert_eq!(text.to_string(), "remote base");
    assert_eq!(
        local.get_list("items").get_deep_value().to_json_value(),
        json!([])
    );
    assert_eq!(
        local.get_map("meta").get_deep_value().to_json_value(),
        json!({})
    );
    assert!(undo.can_redo());
    assert!(undo.redo_count() > 0);

    assert!(undo.redo()?);
    assert_eq!(text.to_string(), "remote base");
    assert_eq!(
        local.get_list("items").get_deep_value().to_json_value(),
        json!(["task"])
    );
    assert_eq!(
        local.get_map("meta").get_deep_value().to_json_value(),
        json!({"tag": "local"})
    );

    let remote_updates = local.export(ExportMode::updates(&remote.oplog_vv()))?;
    remote.import(&remote_updates)?;
    assert_eq!(deep_json(&local), deep_json(&remote));

    Ok(())
}

#[test]
fn undo_callbacks_metadata_limits_and_excluded_origins_follow_contract() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(88)?;
    doc.set_change_merge_interval(0);
    let text = doc.get_text("text");
    let mut undo = UndoManager::new(&doc);
    assert_eq!(undo.peer(), 88);

    undo.add_exclude_origin_prefix("skip");
    doc.set_next_commit_origin("skip:typing");
    text.insert(0, "ignored")?;
    doc.commit();
    assert!(!undo.can_undo());
    assert_eq!(undo.undo_count(), 0);

    let pushed = std::sync::Arc::new(std::sync::Mutex::new(Vec::<UndoOrRedo>::new()));
    let pushed_clone = std::sync::Arc::clone(&pushed);
    undo.set_on_push(Some(Box::new(move |kind, _span, _event| {
        pushed_clone.lock().unwrap().push(kind);
        let mut meta = UndoItemMeta::new();
        meta.set_value(LoroValue::from("meta"));
        meta
    })));

    let popped = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(UndoOrRedo, LoroValue)>::new()));
    let popped_clone = std::sync::Arc::clone(&popped);
    undo.set_on_pop(Some(Box::new(move |kind, _span, meta| {
        popped_clone.lock().unwrap().push((kind, meta.value));
    })));

    text.insert(text.len_unicode(), " one")?;
    doc.commit();
    assert!(undo.can_undo());
    assert_eq!(undo.undo_count(), 1);
    assert_eq!(undo.top_undo_value(), Some(LoroValue::from("meta")));

    assert!(undo.undo()?);
    assert!(undo.can_redo());
    assert_eq!(undo.redo_count(), 1);
    assert_eq!(undo.top_redo_value(), Some(LoroValue::from("meta")));
    assert_eq!(
        popped.lock().unwrap().as_slice(),
        &[(UndoOrRedo::Undo, LoroValue::from("meta"))]
    );

    assert!(undo.redo()?);
    let pushed_events = pushed.lock().unwrap().clone();
    assert!(pushed_events.starts_with(&[UndoOrRedo::Undo, UndoOrRedo::Redo]));
    assert_eq!(undo.top_undo_value(), Some(LoroValue::from("meta")));
    assert_eq!(
        popped.lock().unwrap().as_slice(),
        &[
            (UndoOrRedo::Undo, LoroValue::from("meta")),
            (UndoOrRedo::Redo, LoroValue::from("meta")),
        ]
    );

    undo.set_max_undo_steps(1);
    text.insert(text.len_unicode(), " two")?;
    doc.commit();
    text.insert(text.len_unicode(), " three")?;
    doc.commit();
    assert_eq!(undo.undo_count(), 1);

    undo.clear();
    assert!(!undo.can_undo());
    assert!(!undo.can_redo());
    assert!(!undo.undo()?);
    assert!(!undo.redo()?);

    undo.set_on_push(None);
    undo.set_on_pop(None);
    undo.record_new_checkpoint()?;
    assert!(undo.top_undo_meta().is_none());

    Ok(())
}
