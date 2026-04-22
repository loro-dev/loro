use std::sync::{Arc, Mutex};

use loro::{
    event::Diff, Container, ContainerTrait, ContainerType, ExportMode, LoroCounter, LoroDoc,
    LoroResult, ToJson, VersionVector,
};
use serde_json::json;

#[test]
fn detached_counter_attaches_with_value_and_keeps_public_identity() -> LoroResult<()> {
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
fn counter_events_json_updates_and_deletion_follow_public_contract() -> LoroResult<()> {
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
