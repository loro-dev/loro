use std::borrow::Cow;

use super::gen_action;
use loro::{
    ContainerTrait, ExportMode, Frontiers, LoroDoc, LoroList, LoroMap, LoroMovableList, LoroText,
    LoroValue,
};

#[test]
fn test_snapshot_at_with_multiple_actions() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;

    // Perform a series of actions
    gen_action(&doc, 1, 10);
    doc.commit();
    let frontiers_after_first_commit = doc.oplog_frontiers();
    let value_after_first_commit = doc.get_deep_value();

    gen_action(&doc, 2, 20);
    doc.commit();
    let frontiers_after_second_commit = doc.oplog_frontiers();
    let value_after_second_commit = doc.get_deep_value();
    // Export snapshot at the first frontiers
    let snapshot_at_first = doc.export(ExportMode::SnapshotAt {
        version: Cow::Borrowed(&frontiers_after_first_commit),
    });
    let new_doc_first = LoroDoc::new();
    new_doc_first.import(&snapshot_at_first.unwrap())?;

    // Verify the state of the new document matches the expected state
    assert_eq!(new_doc_first.get_deep_value(), value_after_first_commit);

    // Export snapshot at the second frontiers
    let snapshot_at_second = doc.export(ExportMode::SnapshotAt {
        version: Cow::Borrowed(&frontiers_after_second_commit),
    });
    let new_doc_second = LoroDoc::new();
    new_doc_second.import(&snapshot_at_second.unwrap())?;

    // Verify the state of the new document matches the expected state
    assert_eq!(new_doc_second.get_deep_value(), value_after_second_commit);

    Ok(())
}

#[test]
fn test_fork_at_target_frontiers() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;

    // Perform initial actions
    gen_action(&doc, 1, 10);
    doc.commit();
    let frontiers = doc.oplog_frontiers();
    let value_after_first_commit = doc.get_deep_value();

    // Perform more actions
    gen_action(&doc, 2, 20);
    doc.commit();

    let new_doc = doc.fork_at(&frontiers)?;
    assert_eq!(new_doc.get_deep_value(), value_after_first_commit);

    // Import all updates to the new document
    new_doc.import(&doc.export(ExportMode::all_updates()).unwrap())?;
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());

    Ok(())
}

fn deleted_movable_list_history() -> (LoroDoc, Frontiers, Frontiers, LoroValue) {
    let document = LoroDoc::new();
    document.set_peer_id(1).unwrap();
    let agents = document
        .get_map("root")
        .insert_container("agents", LoroMap::new())
        .unwrap();
    let observer = agents.insert_container("observer", LoroMap::new()).unwrap();
    let permissions = observer
        .insert_container("permissions", LoroMap::new())
        .unwrap();
    let read = permissions
        .insert_container("read", LoroMovableList::new())
        .unwrap();
    read.insert(0, "/resources").unwrap();
    document.commit();
    let seeded = document.oplog_frontiers();
    let seeded_value = document.get_deep_value();

    document.set_peer_id(2).unwrap();
    agents.delete("observer").unwrap();
    document.commit();
    let deleted = document.oplog_frontiers();
    (document, seeded, deleted, seeded_value)
}

#[test]
fn fork_at_head_preserves_deleted_movable_list_history() -> anyhow::Result<()> {
    let (document, seeded, deleted, seeded_value) = deleted_movable_list_history();
    let deleted_value = document.get_deep_value();
    let fork = document.fork_at(&deleted)?;

    assert_eq!(fork.get_deep_value(), deleted_value);
    assert_eq!(fork.oplog_frontiers(), deleted);
    assert!(fork.diff(&seeded, &deleted)?.iter().next().is_some());
    assert!(fork
        .diff(&Frontiers::default(), &seeded)?
        .iter()
        .next()
        .is_some());
    assert_eq!(fork.get_deep_value(), deleted_value);

    fork.checkout(&seeded)?;
    assert_eq!(fork.get_deep_value(), seeded_value);
    fork.checkout(&deleted)?;
    assert_eq!(fork.get_deep_value(), deleted_value);
    assert_eq!(document.state_frontiers(), deleted);
    assert_eq!(document.get_deep_value(), deleted_value);
    assert!(!document.is_detached());
    Ok(())
}

#[test]
fn snapshot_at_past_keeps_deleted_history_but_excludes_future_containers() -> anyhow::Result<()> {
    let (document, seeded, deleted, seeded_value) = deleted_movable_list_history();
    let deleted_value = document.get_deep_value();
    document.set_peer_id(2)?;
    let future = document
        .get_map("root")
        .insert_container("future", LoroMovableList::new())?;
    future.insert(0, "only after the exported frontier")?;
    document.commit();
    let latest = document.oplog_frontiers();
    let latest_value = document.get_deep_value();

    let snapshot = document.export(ExportMode::SnapshotAt {
        version: Cow::Borrowed(&deleted),
    })?;
    let imported = LoroDoc::new();
    imported.import(&snapshot)?;
    assert_eq!(imported.oplog_frontiers(), deleted);
    assert_eq!(imported.get_deep_value(), deleted_value);
    assert!(imported.try_get_movable_list(future.id()).is_none());
    assert!(imported
        .diff(&Frontiers::default(), &seeded)?
        .iter()
        .next()
        .is_some());
    imported.checkout(&seeded)?;
    assert_eq!(imported.get_deep_value(), seeded_value);
    assert_eq!(document.state_frontiers(), latest);
    assert_eq!(document.get_deep_value(), latest_value);
    assert!(!document.is_detached());
    Ok(())
}

#[test]
fn fork_at_deleted_container_preserves_detached_source() -> anyhow::Result<()> {
    let (document, seeded, deleted, seeded_value) = deleted_movable_list_history();
    let deleted_value = document.get_deep_value();
    document.checkout(&seeded)?;
    assert!(document.is_detached());

    let fork = document.fork_at(&deleted)?;
    assert_eq!(fork.get_deep_value(), deleted_value);
    fork.checkout(&seeded)?;
    assert_eq!(fork.get_deep_value(), seeded_value);
    assert_eq!(document.get_deep_value(), seeded_value);
    assert_eq!(document.state_frontiers(), seeded);
    assert!(document.is_detached());
    Ok(())
}

#[test]
fn fork_at_preserves_source_detached_at_head() -> anyhow::Result<()> {
    let (document, _, deleted, _) = deleted_movable_list_history();
    let value = document.get_deep_value();
    document.detach();
    assert!(document.is_detached());
    let fork = document.fork_at(&deleted)?;
    assert_eq!(fork.get_deep_value(), value);
    assert_eq!(document.get_deep_value(), value);
    assert_eq!(document.state_frontiers(), deleted);
    assert!(document.is_detached());
    Ok(())
}

#[test]
fn snapshot_at_keeps_history_of_removed_list_and_text_containers() -> anyhow::Result<()> {
    let document = LoroDoc::new();
    document.set_peer_id(1)?;
    let root = document.get_map("root");
    let parent = root.insert_container("parent", LoroMap::new())?;
    let list = parent.insert_container("list", LoroList::new())?;
    list.push("list value")?;
    let movable = parent.insert_container("movable", LoroMovableList::new())?;
    movable.push("movable value")?;
    let text = parent.insert_container("text", LoroText::new())?;
    text.insert(0, "text value")?;
    document.commit();
    let seeded = document.oplog_frontiers();
    let seeded_value = document.get_deep_value();
    document.set_peer_id(2)?;
    root.delete("parent")?;
    document.commit();
    let deleted = document.oplog_frontiers();
    let deleted_value = document.get_deep_value();

    let fork = document.fork_at(&deleted)?;
    let round_trip = LoroDoc::new();
    round_trip.import(&fork.export(ExportMode::SnapshotAt {
        version: Cow::Borrowed(&deleted),
    })?)?;
    assert!(round_trip
        .diff(&Frontiers::default(), &seeded)?
        .iter()
        .next()
        .is_some());
    round_trip.checkout(&seeded)?;
    assert_eq!(round_trip.get_deep_value(), seeded_value);
    round_trip.checkout(&deleted)?;
    assert_eq!(round_trip.get_deep_value(), deleted_value);
    Ok(())
}
