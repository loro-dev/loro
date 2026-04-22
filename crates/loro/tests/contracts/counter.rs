use std::sync::{Arc, Mutex};

use loro::{
    event::Diff, Container, ContainerTrait, ContainerType, ExportMode, LoroCounter, LoroDoc,
    LoroResult, ToJson, TreeParentId, VersionVector,
};
use serde_json::json;

#[test]
fn detached_counter_attaches_with_value_and_keeps_identity() -> LoroResult<()> {
    let doc = LoroDoc::new();
    let root = doc.get_map("root");
    let detached = LoroCounter::new();

    assert!(!detached.is_attached());
    assert!(detached.doc().is_none());
    assert!(!detached.is_deleted());
    assert_eq!(detached.get(), 0.0);

    detached.increment(4.5)?;
    detached.decrement(1.25)?;
    assert_eq!(detached.get_value(), 3.25);

    let attached = root.insert_container("counter", detached.clone())?;
    assert!(attached.is_attached());
    assert!(attached.doc().is_some());
    assert!(detached.get_attached().is_some());
    assert_eq!(attached.get(), 3.25);
    assert_eq!(detached.get_attached().unwrap().get(), 3.25);

    attached.increment(6.75)?;
    attached.decrement(2.0)?;
    doc.commit();
    assert_eq!(attached.get(), 8.0);
    assert_eq!(
        root.get("counter")
            .unwrap()
            .into_container()
            .unwrap()
            .get_type(),
        ContainerType::Counter
    );
    assert_eq!(
        root.get_deep_value().to_json_value(),
        json!({ "counter": 8.0 })
    );

    let imported = LoroDoc::new();
    imported.import(&doc.export(ExportMode::snapshot())?)?;
    let imported_counter = match imported
        .get_map("root")
        .get("counter")
        .unwrap()
        .into_container()
        .unwrap()
    {
        Container::Counter(counter) => counter,
        other => panic!("expected counter container, got {other:?}"),
    };
    assert_eq!(imported_counter.get(), 8.0);

    Ok(())
}

#[test]
fn counter_events_json_updates_and_deletion_follow_contract() -> LoroResult<()> {
    let doc = LoroDoc::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&events);
    let _sub = doc.subscribe_root(Arc::new(move |event| {
        for container_diff in event.events {
            if let Diff::Counter(delta) = container_diff.diff {
                captured
                    .lock()
                    .unwrap()
                    .push((event.origin.to_string(), delta));
            }
        }
    }));

    let counters = doc.get_list("counters");
    let first = counters.push_container(LoroCounter::new())?;
    first.increment(10.0)?;
    first.decrement(3.0)?;
    doc.set_next_commit_origin("counter-local");
    doc.commit();

    assert_eq!(
        events.lock().unwrap().as_slice(),
        &[("counter-local".to_string(), 7.0)]
    );

    let json_updates = doc.export_json_updates(&VersionVector::default(), &doc.oplog_vv());
    let imported = LoroDoc::new();
    imported.import_json_updates(json_updates)?;
    assert_eq!(
        imported.get_deep_value().to_json_value(),
        doc.get_deep_value().to_json_value()
    );

    let imported_counter = match imported
        .get_list("counters")
        .get(0)
        .unwrap()
        .into_container()
        .unwrap()
    {
        Container::Counter(counter) => counter,
        other => panic!("expected counter container, got {other:?}"),
    };
    assert_eq!(imported_counter.get(), 7.0);

    let root = doc.get_map("root");
    let second = root.insert_container("total", LoroCounter::new())?;
    second.increment(1.5)?;
    doc.commit();
    assert!(!second.is_deleted());
    doc.delete_root_container(root.id());
    doc.commit();
    assert!(second.is_deleted());

    Ok(())
}

#[test]
fn counters_nested_in_tree_meta_and_movable_list_survive_diff_and_snapshot() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(151)?;

    let tree = doc.get_tree("outline");
    let root = tree.create(TreeParentId::Root)?;
    let meta_counter = tree
        .get_meta(root)?
        .insert_container("score", LoroCounter::new())?;
    meta_counter.increment(4.0)?;

    let movable = doc.get_movable_list("totals");
    let list_counter = movable.push_container(LoroCounter::new())?;
    list_counter.increment(2.5)?;
    movable.push("tail")?;
    doc.commit();

    let v1 = doc.state_frontiers();
    let snapshot_v1 = doc.export(ExportMode::Snapshot)?;
    let json_v1 = doc.get_deep_value().to_json_value();
    assert_eq!(json_v1["outline"][0]["meta"]["score"], json!(4.0));
    assert_eq!(json_v1["totals"][0], json!(2.5));

    meta_counter.decrement(1.25)?;
    list_counter.increment(3.5)?;
    movable.mov(1, 0)?;
    doc.commit();
    let v2 = doc.state_frontiers();
    let json_v2 = doc.get_deep_value().to_json_value();
    assert_eq!(json_v2["outline"][0]["meta"]["score"], json!(2.75));
    assert_eq!(json_v2["totals"], json!(["tail", 6.0]));

    let forward = doc.diff(&v1, &v2)?;
    let patched = LoroDoc::from_snapshot(&snapshot_v1)?;
    patched.apply_diff(forward)?;
    assert_eq!(
        patched.get_deep_value().to_json_value(),
        doc.get_deep_value().to_json_value()
    );

    let reverse = doc.diff(&v2, &v1)?;
    let restored = LoroDoc::from_snapshot(&doc.export(ExportMode::Snapshot)?)?;
    restored.apply_diff(reverse)?;
    assert_eq!(restored.get_deep_value().to_json_value(), json_v1);

    let json_updates = doc.export_json_updates(&VersionVector::default(), &doc.oplog_vv());
    let imported = LoroDoc::new();
    imported.import_json_updates(json_updates)?;
    assert_eq!(
        imported.get_deep_value().to_json_value(),
        doc.get_deep_value().to_json_value()
    );

    Ok(())
}
