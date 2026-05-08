use std::{
    borrow::Cow,
    sync::{atomic::AtomicBool, Arc},
};

use super::gen_action;
use loro::{cursor::CannotFindRelativePosition, ExportMode, Frontiers, LoroDoc, VersionVector, ID};

fn multi_frontier_shallow_snapshot() -> anyhow::Result<(Vec<u8>, Frontiers, loro::LoroValue)> {
    let doc = LoroDoc::new();
    doc.set_detached_editing(true);

    doc.set_peer_id(1)?;
    doc.get_text("left").insert(0, "left")?;
    doc.commit();
    let left = doc.state_frontiers();

    doc.checkout(&Frontiers::default())?;
    doc.set_peer_id(2)?;
    doc.get_text("right").insert(0, "right")?;
    doc.commit();
    let right = doc.state_frontiers();

    let mut shallow_root = left.clone();
    shallow_root.merge_with_greater(&right);
    let shallow_root = doc
        .minimize_frontiers(&shallow_root)
        .expect("frontiers should be reachable");
    doc.checkout(&shallow_root)?;
    let expected = doc.get_deep_value();

    let bytes = doc.export(ExportMode::shallow_snapshot(&shallow_root))?;
    Ok((bytes, shallow_root, expected))
}

#[test]
fn state_only_at_concurrent_frontiers_excludes_later_ops() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(0)?;
    doc.set_detached_editing(true);

    doc.get_list("list").insert(0, "Counter")?;
    let list_frontiers = doc.oplog_frontiers();

    doc.checkout(&Frontiers::default())?;
    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let root = tree.create(None)?;
    let mut target_frontiers = list_frontiers;
    target_frontiers.merge_with_greater(&doc.state_frontiers());
    let target_frontiers = doc
        .minimize_frontiers(&target_frontiers)
        .expect("target frontiers should be reachable");

    doc.checkout(&target_frontiers)?;
    let expected = doc.get_deep_value();

    doc.get_tree("tree").create(Some(root))?;
    let latest = doc.get_deep_value();
    assert_ne!(expected, latest);

    let bytes = doc.export(ExportMode::state_only(Some(&target_frontiers)))?;
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes)?;

    assert_eq!(new_doc.get_deep_value(), expected);
    Ok(())
}

#[test]
fn state_only_import_allows_frontiers_that_include_shallow_root() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.set_change_merge_interval(0);

    let text = doc.get_text("text");
    text.insert(0, "root")?;
    doc.commit();
    let shallow_root = doc.state_frontiers();

    doc.set_peer_id(2)?;
    text.insert(text.len_unicode(), " latest")?;
    doc.commit();
    let latest = doc.state_frontiers();
    let expected = doc.get_deep_value();

    let target = Frontiers::from([
        shallow_root.as_single().unwrap(),
        latest.as_single().unwrap(),
    ]);
    let bytes = doc.export(ExportMode::state_only(Some(&target)))?;
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes)?;

    assert!(new_doc.is_shallow());
    assert_eq!(new_doc.shallow_since_frontiers(), shallow_root);
    assert_eq!(new_doc.get_deep_value(), expected);
    Ok(())
}

#[test]
fn checkout_subset_of_multi_frontier_shallow_root_should_error() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_detached_editing(true);

    doc.set_peer_id(1)?;
    doc.get_text("left").insert(0, "left")?;
    doc.commit();
    let left = doc.state_frontiers();

    doc.checkout(&Frontiers::default())?;
    doc.set_peer_id(2)?;
    doc.get_text("right").insert(0, "right")?;
    doc.commit();
    let right = doc.state_frontiers();

    let mut shallow_root = left.clone();
    shallow_root.merge_with_greater(&right);
    let shallow_root = doc
        .minimize_frontiers(&shallow_root)
        .expect("frontiers should be reachable");
    assert_eq!(shallow_root.len(), 2);

    doc.checkout(&shallow_root)?;
    let bytes = doc.export(ExportMode::shallow_snapshot(&shallow_root))?;
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&bytes)?;

    let subset = Frontiers::from([shallow_root.iter().next().unwrap()]);
    assert!(shallow_doc.checkout(&subset).is_err());
    Ok(())
}

#[test]
fn frontiers_to_vv_rejects_unrepresentable_shallow_root_versions() -> anyhow::Result<()> {
    let (bytes, shallow_root, _) = multi_frontier_shallow_snapshot()?;
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&bytes)?;

    let subset = Frontiers::from([shallow_root.iter().next().unwrap()]);
    assert!(shallow_doc.frontiers_to_vv(&Frontiers::default()).is_none());
    assert!(shallow_doc.frontiers_to_vv(&subset).is_none());
    assert!(shallow_doc
        .cmp_frontiers(&Frontiers::default(), &shallow_root)
        .is_err());
    assert!(shallow_doc.cmp_frontiers(&subset, &shallow_root).is_err());
    assert!(shallow_doc.minimize_frontiers(&subset).is_err());
    assert_eq!(
        shallow_doc.cmp_with_frontiers(&Frontiers::default()),
        std::cmp::Ordering::Less
    );
    assert_eq!(
        shallow_doc.cmp_with_frontiers(&subset),
        std::cmp::Ordering::Less
    );

    let shallow_root_vv = shallow_doc
        .frontiers_to_vv(&shallow_root)
        .expect("complete shallow root should be included");
    assert_eq!(shallow_doc.vv_to_frontiers(&shallow_root_vv), shallow_root);
    let mut subset_vv = VersionVector::new();
    subset_vv.set_last(subset.as_single().unwrap());
    assert_eq!(shallow_doc.vv_to_frontiers(&subset_vv), shallow_root);
    assert_eq!(
        shallow_doc
            .cmp_frontiers(&shallow_root, &shallow_root)
            .expect("complete shallow root should be comparable"),
        Some(std::cmp::Ordering::Equal)
    );
    Ok(())
}

#[test]
fn frontiers_to_vv_rejects_shallow_root_deps() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "abcdef")?;
    doc.commit();

    let shallow_root = Frontiers::from_id(ID::new(1, 3));
    let before_root = Frontiers::from_id(ID::new(1, 2));
    let bytes = doc.export(ExportMode::shallow_snapshot(&shallow_root))?;
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&bytes)?;

    assert_eq!(shallow_doc.shallow_since_frontiers(), shallow_root);
    assert!(shallow_doc.checkout(&before_root).is_err());
    assert!(shallow_doc
        .export(ExportMode::shallow_snapshot(&before_root))
        .is_err());
    assert!(shallow_doc
        .export(ExportMode::state_only(Some(&before_root)))
        .is_err());
    assert!(shallow_doc.frontiers_to_vv(&before_root).is_none());
    assert!(shallow_doc
        .cmp_frontiers(&before_root, &shallow_root)
        .is_err());
    assert!(shallow_doc.minimize_frontiers(&before_root).is_err());
    assert_eq!(
        shallow_doc.cmp_with_frontiers(&before_root),
        std::cmp::Ordering::Less
    );
    Ok(())
}

#[test]
fn reexport_multi_frontier_shallow_root_snapshot_imports() -> anyhow::Result<()> {
    let (bytes, shallow_root, expected) = multi_frontier_shallow_snapshot()?;
    let imported = LoroDoc::new();
    imported.import(&bytes)?;

    let reexported = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        imported.export(ExportMode::shallow_snapshot(&shallow_root))
    })) {
        Ok(result) => result?,
        Err(_) => {
            std::mem::forget(imported);
            panic!("re-exporting a multi-frontier shallow root snapshot should not panic");
        }
    };
    let imported_again = LoroDoc::new();
    imported_again.import(&reexported)?;

    assert!(imported_again.is_shallow());
    assert_eq!(imported_again.shallow_since_frontiers(), shallow_root);
    assert_eq!(imported_again.get_deep_value(), expected);
    Ok(())
}

#[test]
fn snapshot_export_preserves_multi_frontier_shallow_root() -> anyhow::Result<()> {
    let (bytes, shallow_root, expected) = multi_frontier_shallow_snapshot()?;
    let imported = LoroDoc::new();
    imported.import(&bytes)?;

    let snapshot = imported.export(ExportMode::Snapshot)?;
    let imported_again = LoroDoc::new();
    imported_again.import(&snapshot)?;

    assert!(imported_again.is_shallow());
    assert_eq!(imported_again.shallow_since_frontiers(), shallow_root);
    assert_eq!(imported_again.get_deep_value(), expected);
    Ok(())
}

#[test]
fn state_only_export_preserves_multi_frontier_shallow_root() -> anyhow::Result<()> {
    let (bytes, shallow_root, expected) = multi_frontier_shallow_snapshot()?;
    let imported = LoroDoc::new();
    imported.import(&bytes)?;

    let state_only = imported.export(ExportMode::state_only(Some(&shallow_root)))?;
    let imported_again = LoroDoc::new();
    imported_again.import(&state_only)?;

    assert!(imported_again.is_shallow());
    assert_eq!(imported_again.shallow_since_frontiers(), shallow_root);
    assert_eq!(imported_again.get_deep_value(), expected);
    Ok(())
}

#[test]
fn state_correctness_check_handles_multi_frontier_shallow_root() -> anyhow::Result<()> {
    let (bytes, _, _) = multi_frontier_shallow_snapshot()?;
    let imported = LoroDoc::new();
    imported.import(&bytes)?;

    imported.check_state_correctness_slow();
    Ok(())
}

#[test]
fn shallow_doc_with_multi_frontier_root_can_export_concurrent_tail() -> anyhow::Result<()> {
    let (bytes, shallow_root, _) = multi_frontier_shallow_snapshot()?;
    let imported = LoroDoc::new();
    imported.import(&bytes)?;
    imported.set_detached_editing(true);

    imported.checkout(&shallow_root)?;
    imported.set_peer_id(3)?;
    imported.get_text("tail_a").insert(0, "a")?;
    imported.get_tree("tail_tree").create(None)?;
    imported.commit();
    let tail_a = imported.state_frontiers();

    imported.checkout(&shallow_root)?;
    imported.set_peer_id(4)?;
    imported.get_text("tail_b").insert(0, "b")?;
    imported.get_tree("tail_tree").create(None)?;
    imported.commit();
    let tail_b = imported.state_frontiers();

    let mut target = tail_a;
    target.merge_with_greater(&tail_b);
    let target = imported
        .minimize_frontiers(&target)
        .expect("tail frontiers should be reachable");
    imported.checkout(&target)?;
    let expected = imported.get_deep_value();

    imported.checkout(&shallow_root)?;
    imported.checkout(&target)?;
    assert_eq!(imported.get_deep_value(), expected);

    let root_to_target = imported.find_id_spans_between(&shallow_root, &target);
    assert!(root_to_target.retreat.is_empty());
    assert!(root_to_target.forward.contains_key(&3));
    assert!(root_to_target.forward.contains_key(&4));

    let clamped_start_to_target = imported.find_id_spans_between(&Frontiers::default(), &target);
    assert_eq!(clamped_start_to_target, root_to_target);

    let target_to_root = imported.find_id_spans_between(&target, &shallow_root);
    assert!(target_to_root.forward.is_empty());
    assert!(target_to_root.retreat.contains_key(&3));
    assert!(target_to_root.retreat.contains_key(&4));

    let target_to_clamped_start = imported.find_id_spans_between(&target, &Frontiers::default());
    assert_eq!(target_to_clamped_start, target_to_root);

    let tail_updates = imported.export(ExportMode::updates_in_range(
        root_to_target.get_id_spans_right().collect::<Vec<_>>(),
    ))?;
    let updated_from_root = LoroDoc::new();
    updated_from_root.import(&bytes)?;
    updated_from_root.import(&tail_updates)?;
    assert_eq!(updated_from_root.get_deep_value(), expected);

    let root_vv = imported
        .frontiers_to_vv(&shallow_root)
        .expect("shallow root should be included");
    let target_vv = imported
        .frontiers_to_vv(&target)
        .expect("target should be included");
    let tail_json = imported.export_json_updates(&root_vv, &target_vv);
    let json_updated_from_root = LoroDoc::new();
    json_updated_from_root.import(&bytes)?;
    json_updated_from_root.import_json_updates(tail_json)?;
    assert_eq!(json_updated_from_root.get_deep_value(), expected);

    let all_tail_json = imported.export_json_updates(&Default::default(), &target_vv);
    let json_all_updated_from_root = LoroDoc::new();
    json_all_updated_from_root.import(&bytes)?;
    json_all_updated_from_root.import_json_updates(all_tail_json)?;
    assert_eq!(json_all_updated_from_root.get_deep_value(), expected);

    let bytes = imported.export(ExportMode::shallow_snapshot(&target))?;
    let imported_again = LoroDoc::new();
    imported_again.import(&bytes)?;

    assert!(imported_again.is_shallow());
    assert_eq!(imported_again.get_deep_value(), expected);

    let state_only = imported.export(ExportMode::state_only(Some(&target)))?;
    let state_only_imported = LoroDoc::new();
    state_only_imported.import(&state_only)?;

    assert!(state_only_imported.is_shallow());
    assert_eq!(state_only_imported.get_deep_value(), expected);

    let latest_state_only = imported.export(ExportMode::state_only(None))?;
    let latest_state_only_imported = LoroDoc::new();
    latest_state_only_imported.import(&latest_state_only)?;

    assert!(latest_state_only_imported.is_shallow());
    assert_eq!(latest_state_only_imported.get_deep_value(), expected);

    let snapshot = imported.export(ExportMode::Snapshot)?;
    let snapshot_imported = LoroDoc::new();
    snapshot_imported.import(&snapshot)?;

    assert!(snapshot_imported.is_shallow());
    assert_eq!(snapshot_imported.get_deep_value(), expected);
    Ok(())
}

#[test]
fn test_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 32);
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    gen_action(&doc, 123, 10);
    doc.commit();
    let shallow_bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));

    let new_doc = LoroDoc::new();
    new_doc.import(&shallow_bytes.unwrap())?;
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());
    Ok(())
}

#[test]
fn test_shallow_1() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "1")?;
    doc.get_text("text").insert(0, "2")?;
    doc.get_text("text").insert(0, "3")?;
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    doc.get_text("text").insert(3, "4")?;
    doc.commit();
    let shallow_bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));

    let new_doc = LoroDoc::new();
    new_doc.import(&shallow_bytes.unwrap())?;
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());
    Ok(())
}

#[test]
fn test_checkout_to_text_that_were_created_before_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "0")?;
    doc.get_text("text").insert(0, "1")?;
    doc.get_text("text").insert(0, "2")?;
    doc.get_text("text").insert(1, "3")?;
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    doc.get_text("text").delete(0, 3)?;
    let bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap())?;
    new_doc.checkout(&frontiers)?;
    assert_eq!(new_doc.get_text("text").to_string(), *"2310");
    Ok(())
}

#[test]
fn test_checkout_to_list_that_were_created_before_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_list("list").insert(0, 0)?;
    doc.get_list("list").insert(1, 1)?;
    doc.get_list("list").insert(2, 2)?;
    doc.get_list("list").insert(1, 3)?;
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    doc.get_list("list").delete(0, 3)?;
    let bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap())?;
    new_doc.checkout(&frontiers)?;
    assert_eq!(
        new_doc.get_list("list").to_vec(),
        vec![0.into(), 3.into(), 1.into(), 2.into()]
    );
    Ok(())
}

#[test]
fn test_checkout_to_movable_list_that_were_created_before_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_movable_list("list").insert(0, 0)?;
    doc.get_movable_list("list").insert(1, 1)?;
    doc.get_movable_list("list").insert(2, 2)?;
    doc.get_movable_list("list").insert(1, 3)?;
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    doc.get_movable_list("list").delete(0, 3)?;
    let bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap())?;
    new_doc.checkout(&frontiers)?;
    assert_eq!(
        new_doc.get_movable_list("list").to_vec(),
        vec![0.into(), 3.into(), 1.into(), 2.into()]
    );
    Ok(())
}

#[test]
fn shallow_on_the_given_version_when_feasible() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 64);
    doc.commit();
    let bytes = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 31)));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap())?;
    assert_eq!(new_doc.shallow_since_vv().get(&1).copied().unwrap(), 31);
    Ok(())
}

#[test]
fn export_snapshot_on_a_shallow_doc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 32);
    doc.commit();

    // Get the current frontiers
    let frontiers = doc.oplog_frontiers();
    let old_value = doc.get_deep_value();
    gen_action(&doc, 123, 32);
    doc.commit();

    // Export using shallowSnapshot mode
    let bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers));

    // Import into a new document
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&bytes.unwrap())?;
    assert_eq!(shallow_doc.shallow_since_vv().get(&1).copied().unwrap(), 31);
    let new_snapshot = shallow_doc.export(loro::ExportMode::Snapshot);

    let new_doc = LoroDoc::new();
    new_doc.import(&new_snapshot.unwrap())?;
    assert_eq!(new_doc.shallow_since_vv().get(&1).copied().unwrap(), 31);
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());
    new_doc.checkout(&frontiers)?;
    assert_eq!(new_doc.get_deep_value(), old_value);
    new_doc.checkout_to_latest();
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());
    Ok(())
}

#[test]
fn export_snapshot_on_shallow_doc_with_small_tail_updates() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 32);
    doc.commit();

    let shallow_frontiers = doc.oplog_frontiers();
    let shallow_value = doc.get_deep_value();
    gen_action(&doc, 456, 4);
    doc.commit();
    let latest_value = doc.get_deep_value();

    let shallow_bytes = doc.export(loro::ExportMode::shallow_snapshot(&shallow_frontiers))?;
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&shallow_bytes)?;

    let snapshot_from_shallow = shallow_doc.export(loro::ExportMode::Snapshot)?;
    let restored = LoroDoc::new();
    restored.import(&snapshot_from_shallow)?;

    assert!(restored.is_shallow());
    assert_eq!(restored.shallow_since_frontiers(), shallow_frontiers);
    assert_eq!(restored.get_deep_value(), latest_value);
    restored.checkout(&shallow_frontiers)?;
    assert_eq!(restored.get_deep_value(), shallow_value);
    Ok(())
}

#[test]
fn test_richtext_gc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let text = doc.get_text("text");
    text.insert(0, "1")?; // 0
    text.insert(0, "2")?; // 1
    text.insert(0, "3")?; // 2
    text.mark(0..2, "bold", "value")?; // 3, 4
    doc.commit();
    text.insert(3, "456")?; // 5, 6, 7
    let bytes = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 3)));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap())?;
    new_doc.checkout(&Frontiers::from(ID::new(1, 4)))?;
    assert_eq!(new_doc.get_text("text").to_string(), "321");
    new_doc.checkout_to_latest();
    assert_eq!(new_doc.get_text("text").to_string(), "321456");
    Ok(())
}

#[test]
fn import_updates_depend_on_shallow_history_should_raise_error() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 4);
    doc.commit();
    let doc2 = doc.fork();
    doc2.get_text("text").insert(0, "1")?;
    doc2.commit();
    gen_action(&doc, 123, 2);
    doc.commit();
    let shallow_snapshot = doc.export(loro::ExportMode::shallow_snapshot(&doc.oplog_frontiers()));
    doc.get_text("hello").insert(0, "world").unwrap();
    doc2.import(
        &doc.export(loro::ExportMode::Updates {
            from: Cow::Borrowed(&doc2.oplog_vv()),
        })
        .unwrap(),
    )
    .unwrap();

    let new_doc = LoroDoc::new();
    new_doc.import(&shallow_snapshot.unwrap()).unwrap();

    let ran = Arc::new(AtomicBool::new(false));
    let ran_clone = ran.clone();
    let _sub = new_doc.subscribe_root(Arc::new(move |e| {
        ran_clone.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(e.events.len() == 1);
        match e.events[0].diff {
            loro::event::Diff::Text(_) => {}
            _ => {
                unreachable!()
            }
        }
    }));
    let result = new_doc.import(
        &doc2
            .export(loro::ExportMode::updates_owned(new_doc.oplog_vv()))
            .unwrap(),
    );
    assert!(result.is_err());
    // But updates from doc should be fine ("hello": "world")
    assert_eq!(new_doc.get_text("hello").to_string(), *"world");
    assert!(ran.load(std::sync::atomic::Ordering::Relaxed));
    Ok(())
}

#[test]
fn the_vv_on_shallow_doc() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    gen_action(&doc, 0, 10);
    doc.commit();
    let snapshot = doc.export(loro::ExportMode::shallow_snapshot(&doc.oplog_frontiers()));
    let new_doc = LoroDoc::new();
    new_doc.import(&snapshot.unwrap()).unwrap();
    assert!(!new_doc.shallow_since_vv().is_empty());
    assert_eq!(new_doc.oplog_vv(), new_doc.state_vv());
    assert_eq!(new_doc.oplog_vv(), doc.state_vv());
    assert_eq!(new_doc.oplog_frontiers(), doc.oplog_frontiers());
    assert_eq!(new_doc.oplog_frontiers(), new_doc.state_frontiers());
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());

    gen_action(&doc, 0, 10);
    doc.commit();
    let bytes = doc.export(ExportMode::all_updates());
    new_doc.import(&bytes.unwrap()).unwrap();
    assert_eq!(new_doc.oplog_vv(), new_doc.state_vv());
    assert_eq!(new_doc.oplog_vv(), doc.state_vv());
    assert_eq!(new_doc.oplog_frontiers(), doc.oplog_frontiers());
    assert_eq!(new_doc.oplog_frontiers(), new_doc.state_frontiers());
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());

    Ok(())
}

#[test]
fn no_event_when_exporting_shallow_snapshot() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 0, 10);
    doc.commit();
    let _id = doc.subscribe_root(Arc::new(|_diff| {
        panic!("should not emit event");
    }));
    let _snapshot = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 3)));
    Ok(())
}

#[test]
fn test_cursor_that_cannot_be_found_when_exporting_shallow_snapshot() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "Hello world")?;
    let c = doc
        .get_text("text")
        .get_cursor(3, loro::cursor::Side::Left)
        .unwrap();
    doc.get_text("text").delete(0, 5)?;
    doc.commit();
    let snapshot = doc.export(loro::ExportMode::shallow_snapshot(&doc.oplog_frontiers()));
    let new_doc = LoroDoc::new();
    new_doc.import(&snapshot.unwrap())?;
    let result = new_doc.get_cursor_pos(&c);
    match result {
        Ok(v) => {
            dbg!(v);
            unreachable!()
        }
        Err(CannotFindRelativePosition::HistoryCleared) => {}
        Err(x) => {
            dbg!(x);
            unreachable!()
        }
    }
    Ok(())
}

#[test]
fn test_cursor_that_can_be_found_when_exporting_shallow_snapshot() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "Hello world")?;
    doc.commit();
    let c = doc
        .get_text("text")
        .get_cursor(3, loro::cursor::Side::Left)
        .unwrap();
    doc.get_text("text").delete(0, 5)?;
    doc.commit();
    let snapshot = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 10)));
    let new_doc = LoroDoc::new();
    new_doc.import(&snapshot.unwrap())?;
    let result = new_doc.get_cursor_pos(&c);
    match result {
        Ok(v) => {
            assert_eq!(v.current.pos, 0);
        }
        Err(x) => {
            dbg!(x);
            unreachable!()
        }
    }
    Ok(())
}

#[test]
fn test_export_shallow_snapshot_from_shallow_doc() -> anyhow::Result<()> {
    // Create and populate the original document
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    gen_action(&doc, 123, 32);
    doc.commit();

    // Get the current frontiers and create some more actions
    let frontiers = doc.oplog_frontiers();
    gen_action(&doc, 123, 32);
    doc.commit();

    // Export using shallowSnapshot mode
    let shallow_bytes = doc.export(loro::ExportMode::shallow_snapshot(&frontiers))?;

    // Import into a new document
    let shallow_doc = LoroDoc::new();
    shallow_doc.import(&shallow_bytes)?;

    // Attempt to export a shallow snapshot from the shallow document
    // using frontiers before its shallow version
    let result = shallow_doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 16)));

    // The export should fail because the requested frontiers are before the shallow version
    assert!(result.is_err());

    if let Err(e) = result {
        assert!(matches!(e, loro::LoroEncodeError::FrontiersNotFound(..)));
    } else {
        panic!("Expected an error, but got Ok");
    }

    Ok(())
}
