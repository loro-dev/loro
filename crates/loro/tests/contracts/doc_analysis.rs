use std::ops::ControlFlow;

use loro::{
    ChangeTravelError, CommitOptions, ContainerTrait, ExportMode, Frontiers, Index, LoroDoc,
    LoroList, LoroMap, LoroText, ToJson, TreeParentId, ID,
};
use pretty_assertions::assert_eq;
use serde_json::json;

fn json_value(doc: &LoroDoc) -> serde_json::Value {
    doc.get_deep_value().to_json_value()
}

#[test]
fn doc_analysis_compaction_and_state_correctness_follow_contract() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(101)?;
    doc.set_record_timestamp(true);
    doc.set_change_merge_interval(0);

    let map = doc.get_map("root");
    map.insert("title", "draft")?;
    let text = map.insert_container("text", LoroText::new())?;
    text.insert(0, "hello")?;
    let list = map.insert_container("items", LoroList::new())?;
    list.push("one")?;
    list.push("two")?;
    doc.commit_with(CommitOptions::new().timestamp(10).commit_msg("seed"));

    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let root = tree.create(TreeParentId::Root)?;
    tree.get_meta(root)?.insert("kind", "root")?;
    doc.commit_with(CommitOptions::new().timestamp(20).commit_msg("tree"));

    let nested = list.push_container(LoroMap::new())?;
    nested.insert("name", "nested")?;
    text.insert(text.len_unicode(), " world")?;
    doc.commit_with(CommitOptions::new().timestamp(30).commit_msg("extend"));

    let before_compact = json_value(&doc);
    let analysis = doc.analyze();
    assert!(!analysis.is_empty());
    assert_eq!(analysis.dropped_len(), 0);
    assert!(analysis.tiny_container_len() > 0);
    assert!(
        analysis
            .containers
            .get(&map.id())
            .expect("root map should be analyzed")
            .ops_num
            > 0
    );
    assert!(
        analysis
            .containers
            .get(&text.id())
            .expect("text should be analyzed")
            .last_edit_time
            >= 30
    );

    doc.check_state_correctness_slow();
    doc.compact_change_store();
    assert_eq!(json_value(&doc), before_compact);
    doc.check_state_correctness_slow();

    let snapshot = doc.export(ExportMode::Snapshot)?;
    let imported = LoroDoc::from_snapshot(&snapshot)?;
    imported.check_state_correctness_slow();
    assert_eq!(json_value(&imported), before_compact);

    Ok(())
}

#[test]
fn checkout_history_cache_and_diff_application_follow_contract() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(102)?;
    doc.set_change_merge_interval(0);

    let text = doc.get_text("text");
    let list = doc.get_list("list");
    let map = doc.get_map("map");

    let v0 = doc.state_frontiers();
    text.insert(0, "abc")?;
    list.push("a")?;
    map.insert("phase", "one")?;
    doc.commit();
    let v1 = doc.state_frontiers();

    text.insert(1, "XYZ")?;
    list.insert(1, "b")?;
    map.insert("phase", "two")?;
    doc.commit();
    let v2 = doc.state_frontiers();
    let expected_v2 = json_value(&doc);

    let diff_v0_v2 = doc.diff(&v0, &v2)?;
    let replay = LoroDoc::new();
    replay.apply_diff(diff_v0_v2)?;
    assert_eq!(json_value(&replay), expected_v2);

    doc.checkout(&v1)?;
    assert!(doc.is_detached());
    assert_eq!(
        json_value(&doc),
        json!({"list": ["a"], "map": {"phase": "one"}, "text": "abc"})
    );
    assert!(doc.has_history_cache());

    doc.free_history_cache();
    assert!(!doc.has_history_cache());
    doc.free_diff_calculator();

    doc.checkout_to_latest();
    assert!(!doc.is_detached());
    assert_eq!(json_value(&doc), expected_v2);

    doc.revert_to(&v1)?;
    assert_eq!(
        json_value(&doc),
        json!({"list": ["a"], "map": {"phase": "one"}, "text": "abc"})
    );

    Ok(())
}

#[test]
fn change_traversal_exposes_changed_containers_and_id_spans() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(103)?;
    doc.set_change_merge_interval(0);

    let root = doc.get_map("root");
    root.insert("title", "first")?;
    doc.commit_with(CommitOptions::new().commit_msg("first"));
    let first = doc
        .state_frontiers()
        .as_single()
        .expect("single local commit frontier");
    let first_change = doc.get_change(first).expect("first change should exist");

    let text = root.insert_container("body", LoroText::new())?;
    text.insert(0, "hello")?;
    doc.commit_with(CommitOptions::new().commit_msg("second"));
    let second = doc
        .state_frontiers()
        .as_single()
        .expect("single local commit frontier");
    let second_change = doc.get_change(second).expect("second change should exist");

    let list = root.insert_container("items", LoroList::new())?;
    list.push("a")?;
    doc.commit_with(CommitOptions::new().commit_msg("third"));
    let third = doc
        .state_frontiers()
        .as_single()
        .expect("single local commit frontier");

    let changed_first = doc.get_changed_containers_in(first_change.id, first_change.len);
    assert!(changed_first.contains(&root.id()));

    let changed_second = doc.get_changed_containers_in(second_change.id, second_change.len);
    assert!(changed_second.contains(&text.id()));

    let between = doc.find_id_spans_between(&Frontiers::from_id(first), &Frontiers::from_id(third));
    assert_eq!(
        between.forward.get(&103).map(|span| (span.start, span.end)),
        Some((first.counter + 1, third.counter + 1))
    );

    let mut messages = Vec::new();
    doc.travel_change_ancestors(&[third], &mut |change| {
        messages.push(change.message().to_string());
        ControlFlow::Continue(())
    })?;
    assert_eq!(messages, vec!["third", "second", "first"]);

    let mut first_two = Vec::new();
    doc.travel_change_ancestors(&[third], &mut |change| {
        first_two.push(change.message().to_string());
        if first_two.len() == 2 {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })?;
    assert_eq!(first_two, vec!["third", "second"]);

    Ok(())
}

#[test]
fn version_comparison_travel_errors_and_snapshot_mode_contracts() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(106)?;
    doc.set_change_merge_interval(0);

    let root = doc.get_map("root");
    assert_eq!(doc.get_pending_txn_len(), 0);
    root.insert("phase", "one")?;
    assert_eq!(doc.get_pending_txn_len(), 1);
    let exported_from_pending_txn = doc.export(ExportMode::all_updates())?;
    assert_eq!(doc.get_pending_txn_len(), 0);
    assert!(LoroDoc::from_snapshot(&exported_from_pending_txn).is_err());
    let first = doc.state_frontiers();
    let first_id = first.as_single().expect("first commit frontier");

    root.insert("phase", "two")?;
    doc.commit_with(CommitOptions::new().commit_msg("second"));
    let second = doc.state_frontiers();
    let second_id = second.as_single().expect("second commit frontier");

    assert_eq!(
        doc.cmp_frontiers(&first, &second)
            .expect("frontiers should be included"),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(doc.cmp_with_frontiers(&second), std::cmp::Ordering::Equal);
    assert!(doc
        .cmp_frontiers(&Frontiers::from_id(ID::new(999, 0)), &second)
        .is_err());

    let missing =
        doc.travel_change_ancestors(&[ID::new(106, 999)], &mut |_| ControlFlow::Continue(()));
    assert!(matches!(
        missing,
        Err(ChangeTravelError::TargetIdNotFound(_))
    ));

    let shallow_blob = doc.export(ExportMode::shallow_snapshot(&second))?;
    let shallow = LoroDoc::new();
    shallow.import(&shallow_blob)?;
    let shallow_err =
        shallow.travel_change_ancestors(&[first_id], &mut |_| ControlFlow::Continue(()));
    assert!(matches!(
        shallow_err,
        Err(ChangeTravelError::TargetVersionNotIncluded)
    ));

    let mut visited = Vec::new();
    doc.travel_change_ancestors(&[second_id], &mut |change| {
        visited.push(change.id);
        ControlFlow::Continue(())
    })?;
    assert_eq!(visited.first(), Some(&second_id));
    assert!(visited.contains(&first_id));

    Ok(())
}

#[test]
fn shallow_doc_state_check_and_export_boundaries_follow_contract() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(104)?;
    doc.set_change_merge_interval(0);

    let text = doc.get_text("text");
    text.insert(0, "alpha")?;
    doc.commit_with(CommitOptions::new().timestamp(1));
    let shallow_start = doc.state_frontiers();

    text.insert(text.len_unicode(), "-beta")?;
    doc.get_map("meta").insert("stage", "beta")?;
    doc.commit_with(CommitOptions::new().timestamp(2));
    let expected_latest = json_value(&doc);

    let shallow_blob = doc.export(ExportMode::shallow_snapshot(&shallow_start))?;
    let shallow = LoroDoc::new();
    shallow.import(&shallow_blob)?;
    assert!(shallow.is_shallow());
    assert_eq!(shallow.shallow_since_frontiers(), shallow_start);
    assert_eq!(json_value(&shallow), expected_latest);
    shallow.check_state_correctness_slow();

    let latest_updates = doc.export(ExportMode::updates(&shallow.shallow_since_vv().to_vv()))?;
    let imported = LoroDoc::new();
    imported.import(&shallow_blob)?;
    imported.import(&latest_updates)?;
    assert_eq!(json_value(&imported), expected_latest);

    Ok(())
}

#[test]
fn path_queries_and_root_values_preserve_container_contracts() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    let root = doc.get_map("root");
    let list = root.insert_container("items", LoroList::new())?;
    let child = list.push_container(LoroMap::new())?;
    child.insert("name", "leaf")?;
    let text = child.insert_container("body", LoroText::new())?;
    text.insert(0, "content")?;

    assert_eq!(
        doc.get_by_path(&[
            Index::Key("root".into()),
            Index::Key("items".into()),
            Index::Seq(0),
            Index::Key("name".into())
        ])
        .unwrap()
        .into_value()
        .unwrap(),
        "leaf".into()
    );
    assert_eq!(
        doc.get_by_str_path("root/items/0/body")
            .unwrap()
            .into_container()
            .unwrap()
            .id(),
        text.id()
    );
    assert!(doc.get_by_str_path("root/items/9").is_none());
    assert!(doc.get_by_str_path("root/items/0/body/missing").is_none());

    let shallow = doc.get_value().to_json_value();
    let deep = doc.get_deep_value().to_json_value();
    let deep_with_id = doc.get_deep_value_with_id().to_json_value();
    assert_ne!(shallow, deep);
    assert_eq!(
        deep,
        json!({"root": {"items": [{"body": "content", "name": "leaf"}]}})
    );
    assert!(deep_with_id["root"]["cid"].is_string());
    assert_eq!(
        deep_with_id["root"]["value"]["items"]["value"][0]["value"]["body"]["value"],
        json!("content")
    );

    Ok(())
}

#[test]
fn deleted_containers_snapshot_and_compaction_follow_contract() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(108)?;
    doc.set_change_merge_interval(0);

    let root = doc.get_map("root");
    let body = root.insert_container("body", LoroText::new())?;
    body.insert(0, "hello")?;
    let items = root.insert_container("items", LoroList::new())?;
    items.push("one")?;
    items.push("two")?;
    doc.commit();

    let body_id = body.id();
    let items_id = items.id();
    assert_eq!(
        doc.get_path_to_container(&body_id)
            .unwrap()
            .last()
            .unwrap()
            .1,
        Index::Key("body".into())
    );
    assert_eq!(
        doc.get_path_to_container(&items_id)
            .unwrap()
            .last()
            .unwrap()
            .1,
        Index::Key("items".into())
    );

    root.delete("body")?;
    root.delete("items")?;
    doc.commit();

    let expected = json_value(&doc);
    assert_eq!(expected, json!({"root": {}}));
    doc.check_state_correctness_slow();

    let snapshot = doc.export(ExportMode::Snapshot)?;
    let restored = LoroDoc::from_snapshot(&snapshot)?;
    assert_eq!(json_value(&restored), expected);
    restored.check_state_correctness_slow();

    restored.compact_change_store();
    assert_eq!(json_value(&restored), expected);
    restored.check_state_correctness_slow();

    let shallow = doc.export(ExportMode::state_only(Some(&doc.state_frontiers())))?;
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&shallow)?;
    assert!(shallow_doc.is_shallow());
    assert_eq!(json_value(&shallow_doc), expected);

    Ok(())
}
