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

#[test]
fn undo_text_delete_is_independent_of_remote_import_granularity() -> LoroResult<()> {
    fn run(split: bool) -> LoroResult<String> {
        let doc = LoroDoc::new();
        doc.set_peer_id(1)?;
        let remote = LoroDoc::new();
        remote.set_peer_id(2)?;
        let text = doc.get_text("t");
        let other = remote.get_text("t");

        text.insert(0, "ab")?;
        doc.commit();
        remote.import(&doc.export(ExportMode::Snapshot)?)?;
        other.insert(0, "r")?;
        remote.commit();
        doc.import(&remote.export(ExportMode::Snapshot)?)?;
        assert_eq!(text.to_string(), "rab");

        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);

        text.delete(1, 1)?; // A deletes "a" -> "rb"
        doc.commit();

        other.insert(0, "s")?;
        remote.commit();
        if split {
            doc.import(&remote.export(ExportMode::Snapshot)?)?;
        }
        other.delete(1, 1)?; // B deletes "r" -> "sab"
        remote.commit();
        doc.import(&remote.export(ExportMode::Snapshot)?)?;
        assert_eq!(text.to_string(), "sb");

        assert!(undo.undo()?);
        Ok(text.to_string())
    }

    assert_eq!(run(true)?, "sab");
    assert_eq!(run(false)?, "sab");
    Ok(())
}

#[test]
fn undo_text_and_list_delete_variants_are_independent_of_remote_import_granularity(
) -> LoroResult<()> {
    // Variant 1: remote inserts after "r", then deletes "r" (different order from #1297).
    fn run_insert_after_then_delete(split: bool) -> LoroResult<String> {
        let doc = LoroDoc::new();
        doc.set_peer_id(1)?;
        let remote = LoroDoc::new();
        remote.set_peer_id(2)?;
        let text = doc.get_text("t");
        let other = remote.get_text("t");

        text.insert(0, "rab")?;
        doc.commit();
        remote.import(&doc.export(ExportMode::Snapshot)?)?;

        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);

        text.delete(1, 1)?; // "rb"
        doc.commit();

        other.insert(1, "s")?; // "rsab"
        remote.commit();
        if split {
            doc.import(&remote.export(ExportMode::Snapshot)?)?;
        }
        other.delete(0, 1)?; // "sab"
        remote.commit();
        doc.import(&remote.export(ExportMode::Snapshot)?)?;
        assert_eq!(text.to_string(), "sb");

        assert!(undo.undo()?);
        Ok(text.to_string())
    }

    assert_eq!(run_insert_after_then_delete(true)?, "sab");
    assert_eq!(run_insert_after_then_delete(false)?, "sab");

    // Variant 2: remote deletes "r" first, then inserts "s" at 0.
    fn run_delete_then_insert(split: bool) -> LoroResult<String> {
        let doc = LoroDoc::new();
        doc.set_peer_id(1)?;
        let remote = LoroDoc::new();
        remote.set_peer_id(2)?;
        let text = doc.get_text("t");
        let other = remote.get_text("t");

        text.insert(0, "rab")?;
        doc.commit();
        remote.import(&doc.export(ExportMode::Snapshot)?)?;

        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);

        text.delete(1, 1)?; // "rb"
        doc.commit();

        other.delete(0, 1)?; // "ab"
        remote.commit();
        if split {
            doc.import(&remote.export(ExportMode::Snapshot)?)?;
        }
        other.insert(0, "s")?; // "sab"
        remote.commit();
        doc.import(&remote.export(ExportMode::Snapshot)?)?;
        assert_eq!(text.to_string(), "sb");

        assert!(undo.undo()?);
        Ok(text.to_string())
    }

    assert_eq!(run_delete_then_insert(true)?, "sab");
    assert_eq!(run_delete_then_insert(false)?, "sab");

    // Variant 3: remote inserts multiple styled runs and deletes "r", plus cursor in UndoItemMeta.
    fn run_styled_with_cursor(split: bool) -> LoroResult<(String, usize)> {
        let doc = LoroDoc::new();
        doc.set_peer_id(1)?;
        let remote = LoroDoc::new();
        remote.set_peer_id(2)?;
        let text = doc.get_text("t");
        let other = remote.get_text("t");

        text.insert(0, "rab")?;
        doc.commit();
        remote.import(&doc.export(ExportMode::Snapshot)?)?;

        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);
        let popped_pos = std::sync::Arc::new(std::sync::Mutex::new(None));
        let popped_pos_clone = std::sync::Arc::clone(&popped_pos);
        let text_for_push = text.clone();
        undo.set_on_push(Some(Box::new(move |_, _, _| {
            let mut meta = UndoItemMeta::new();
            if let Some(c) = text_for_push.get_cursor(1, Side::Left) {
                meta.add_cursor(&c);
            }
            meta
        })));
        undo.set_on_pop(Some(Box::new(move |_, _, meta| {
            *popped_pos_clone.lock().unwrap() = meta.cursors.first().map(|c| c.pos.pos);
        })));

        text.delete(1, 1)?; // "rb"
        doc.commit();

        other.insert(0, "s")?;
        other.mark(0..1, "bold", true)?;
        remote.commit();
        if split {
            doc.import(&remote.export(ExportMode::Snapshot)?)?;
        }
        other.insert(1, "t")?;
        other.mark(1..2, "italic", true)?;
        remote.commit();
        if split {
            doc.import(&remote.export(ExportMode::Snapshot)?)?;
        }
        other.delete(2, 1)?; // deletes "r"
        remote.commit();
        doc.import(&remote.export(ExportMode::Snapshot)?)?;
        assert_eq!(text.to_string(), "stb");

        assert!(undo.undo()?);
        let pos = popped_pos.lock().unwrap().expect("cursor should be popped");
        Ok((text.to_string(), pos))
    }

    assert_eq!(run_styled_with_cursor(true)?, ("stab".to_string(), 2));
    assert_eq!(run_styled_with_cursor(false)?, ("stab".to_string(), 2));

    // Variant 4: LoroList has the same DeltaRope transform path.
    fn run_list(split: bool) -> LoroResult<Value> {
        let doc = LoroDoc::new();
        doc.set_peer_id(1)?;
        let remote = LoroDoc::new();
        remote.set_peer_id(2)?;
        let list = doc.get_list("l");
        let other = remote.get_list("l");

        list.insert(0, "r")?;
        list.insert(1, "a")?;
        list.insert(2, "b")?;
        doc.commit();
        remote.import(&doc.export(ExportMode::Snapshot)?)?;

        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);

        list.delete(1, 1)?; // ["r", "b"]
        doc.commit();

        other.insert(0, "s")?;
        remote.commit();
        if split {
            doc.import(&remote.export(ExportMode::Snapshot)?)?;
        }
        other.delete(1, 1)?; // deletes "r"
        remote.commit();
        doc.import(&remote.export(ExportMode::Snapshot)?)?;
        assert_eq!(list.get_deep_value().to_json_value(), json!(["s", "b"]));

        assert!(undo.undo()?);
        Ok(list.get_deep_value().to_json_value())
    }

    assert_eq!(run_list(true)?, json!(["s", "a", "b"]));
    assert_eq!(run_list(false)?, json!(["s", "a", "b"]));

    Ok(())
}

#[test]
fn undo_text_randomized_concurrent_edits_are_independent_of_import_granularity() -> LoroResult<()> {
    fn next_u64(state: &mut u64) -> u64 {
        *state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *state
    }

    fn run(seed: u64, split: bool) -> LoroResult<(String, String)> {
        let mut rng = seed;
        let doc = LoroDoc::new();
        doc.set_peer_id(1)?;
        let remote = LoroDoc::new();
        remote.set_peer_id(2)?;
        let text = doc.get_text("t");
        let other = remote.get_text("t");

        text.insert(0, "abcdef")?;
        doc.commit();
        remote.import(&doc.export(ExportMode::Snapshot)?)?;

        let mut undo = UndoManager::new(&doc);
        undo.set_merge_interval(0);

        // 1-2 local edits on A
        let local_steps = 1 + (next_u64(&mut rng) % 2) as usize;
        for step in 0..local_steps {
            let len = text.len_unicode();
            if len > 0 && next_u64(&mut rng) % 2 == 0 {
                let pos = (next_u64(&mut rng) as usize) % len;
                let del_len = 1 + ((next_u64(&mut rng) as usize) % (len - pos).min(2));
                text.delete(pos, del_len)?;
            } else {
                let pos = (next_u64(&mut rng) as usize) % (len + 1);
                let ch = (b'A' + (step as u8 % 26)) as char;
                text.insert(pos, &ch.to_string())?;
            }
            doc.commit();
        }

        // 2-5 concurrent remote commits on B
        let remote_steps = 2 + (next_u64(&mut rng) % 4) as usize;
        for step in 0..remote_steps {
            let len = other.len_unicode();
            if len > 0 && next_u64(&mut rng) % 2 == 0 {
                let pos = (next_u64(&mut rng) as usize) % len;
                let del_len = 1 + ((next_u64(&mut rng) as usize) % (len - pos).min(2));
                other.delete(pos, del_len)?;
            } else {
                let pos = (next_u64(&mut rng) as usize) % (len + 1);
                let ch = (b'0' + (step as u8 % 10)) as char;
                other.insert(pos, &ch.to_string())?;
            }
            remote.commit();
            if split {
                doc.import(&remote.export(ExportMode::Snapshot)?)?;
            }
        }
        if !split {
            doc.import(&remote.export(ExportMode::Snapshot)?)?;
        }

        while undo.can_undo() {
            let _ = undo.undo()?;
        }
        let after_undo = text.to_string();

        while undo.can_redo() {
            let _ = undo.redo()?;
        }
        let after_redo = text.to_string();

        Ok((after_undo, after_redo))
    }

    for seed in 1..=100 {
        assert_eq!(
            run(seed, false)?,
            run(seed, true)?,
            "mismatch for seed {seed}"
        );
    }

    Ok(())
}
