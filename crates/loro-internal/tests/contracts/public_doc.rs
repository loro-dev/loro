use std::sync::{Arc, Mutex};

use loro_common::{ContainerID, ContainerType, LoroResult, TreeID, ID};
use loro_internal::{
    cursor::Side,
    event::{path_to_str, str_to_path, EventTriggerKind, Index},
    handler::{Handler, ListHandler, MapHandler, MovableListHandler, TextHandler, ValueOrHandler},
    loro::ExportMode,
    version::VersionVector,
    HandlerTrait, LoroDoc, ToJson, TreeParentId,
};
use pretty_assertions::assert_eq;
use serde_json::{json, Value};

fn deep_json(doc: &LoroDoc) -> Value {
    doc.get_deep_value().to_json_value()
}

fn value_or_handler_deep_json(value: ValueOrHandler) -> Value {
    match value {
        ValueOrHandler::Value(value) => value.to_json_value(),
        ValueOrHandler::Handler(handler) => handler.get_deep_value().to_json_value(),
    }
}

#[test]
fn internal_doc_introspection_uses_the_same_container_path_contracts() -> LoroResult<()> {
    let pristine = LoroDoc::new();
    assert!(pristine.can_reset_with_snapshot());

    let doc = LoroDoc::new_auto_commit();
    doc.set_peer_id(901)?;

    let root = doc.get_map("root");
    root.insert("title", "draft")?;

    let body = root.insert_container("body", TextHandler::new_detached())?;
    body.insert_unicode(0, "hello")?;

    let list = root.insert_container("items", ListHandler::new_detached())?;
    list.insert(0, "one")?;
    let nested_map = list.insert_container(1, MapHandler::new_detached())?;
    nested_map.insert("kind", "nested")?;

    let movable = root.insert_container("order", MovableListHandler::new_detached())?;
    movable.insert(0, "first")?;
    movable.insert(1, "second")?;
    movable.mov(1, 0)?;

    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let root_node = tree.create(TreeParentId::Root)?;
    tree.get_meta(root_node)?.insert("label", "root")?;

    let uncommitted = doc
        .get_uncommitted_ops_as_json()
        .expect("pending local ops should be visible before commit");
    assert!(!uncommitted.changes.is_empty());

    doc.commit_then_renew();
    assert!(doc.get_uncommitted_ops_as_json().is_none());
    assert!(!doc.can_reset_with_snapshot());

    let value_with_ids = doc.app_state().lock().get_deep_value_with_id();
    let value_with_ids_json = value_with_ids.to_json_value();
    assert_eq!(
        value_with_ids_json["root"]["value"]["title"],
        json!("draft")
    );
    assert!(value_with_ids_json["root"]["cid"]
        .as_str()
        .expect("cid should be a string")
        .contains("cid:"));

    let flat = doc.app_state().lock().get_all_container_value_flat();
    let flat_json = flat.to_json_value();
    assert_eq!(flat_json[body.id().to_string()], json!("hello"));
    assert_eq!(
        flat_json[nested_map.id().to_string()],
        json!({"kind": "nested"})
    );

    let body_path = doc
        .get_path_to_container(&body.id())
        .expect("body should have a root path");
    let indexes = body_path
        .iter()
        .map(|(_, index)| index.clone())
        .collect::<Vec<_>>();
    assert_eq!(path_to_str(&indexes), "root/body");
    assert_eq!(str_to_path("root/body"), Some(indexes.clone()));
    assert_eq!(
        value_or_handler_deep_json(doc.get_by_path(&indexes).expect("body path should resolve")),
        json!("hello")
    );
    assert!(doc
        .get_path_to_container(&ContainerID::new_normal(
            ID::new(777, 0),
            ContainerType::Text
        ))
        .is_none());

    assert_eq!(EventTriggerKind::Local.to_string(), "local");
    assert_eq!(EventTriggerKind::Import.to_string(), "import");
    assert_eq!(EventTriggerKind::Checkout.to_string(), "checkout");
    assert!(EventTriggerKind::Local.is_local());
    assert!(EventTriggerKind::Import.is_import());
    assert!(EventTriggerKind::Checkout.is_checkout());

    assert_eq!(Index::try_from("").unwrap(), Index::Key("".into()));
    assert_eq!(Index::try_from("12").unwrap(), Index::Seq(12));
    assert_eq!(Index::try_from("12x").unwrap(), Index::Key("12x".into()));
    assert_eq!(Index::Node(TreeID::new(5, 7)).to_string(), "5@7");
    assert_eq!(
        format!("{:?}", Index::Key("plain".into())),
        "Index::Key(\"plain\")"
    );
    assert_eq!(
        str_to_path("root/1/5@7"),
        Some(vec![
            Index::Key("root".into()),
            Index::Seq(1),
            Index::Node(TreeID::new(7, 5)),
        ])
    );

    let fork = doc.fork();
    assert_eq!(deep_json(&fork), deep_json(&doc));
    doc.detach();
    let detached_fork = doc.fork();
    assert_eq!(deep_json(&detached_fork), deep_json(&doc));
    doc.attach();

    let text_cursor = body
        .get_cursor(0, Side::Middle)
        .expect("text cursor should be created");
    assert_eq!(
        doc.query_pos(&text_cursor)
            .expect("cursor should resolve")
            .current
            .pos,
        0
    );

    Ok(())
}

#[test]
fn import_diff_apply_and_checkout_events_follow_doc_contracts() -> LoroResult<()> {
    let source = LoroDoc::new_auto_commit();
    source.set_peer_id(902)?;

    let source_events = Arc::new(Mutex::new(Vec::<(EventTriggerKind, String, usize)>::new()));
    let source_events_ref = Arc::clone(&source_events);
    let _source_sub = source.subscribe_root(Arc::new(move |event| {
        source_events_ref.lock().unwrap().push((
            event.event_meta.by,
            event.event_meta.origin.to_string(),
            event.events.len(),
        ));
    }));

    let text = source.get_text("text");
    text.insert_unicode(0, "a")?;
    source.set_next_commit_origin("seed");
    source.commit_then_renew();
    let v1 = source.state_frontiers();

    text.insert_unicode(1, "b")?;
    let list = source.get_list("list");
    list.insert(0, "item")?;
    let nested = list.insert_container(1, TextHandler::new_detached())?;
    nested.insert_unicode(0, "nested")?;
    source.set_next_commit_origin("second");
    source.commit_then_renew();
    let v2 = source.state_frontiers();

    source.checkout(&v1)?;
    assert_eq!(deep_json(&source), json!({"text": "a", "list": []}));
    source.checkout_to_latest();
    assert_eq!(
        deep_json(&source),
        json!({"text": "ab", "list": ["item", "nested"]})
    );

    let observed = source_events.lock().unwrap().clone();
    assert!(observed
        .iter()
        .any(|(kind, origin, _)| kind.is_local() && origin == "seed"));
    assert!(observed
        .iter()
        .any(|(kind, origin, _)| kind.is_local() && origin == "second"));
    assert!(observed.iter().any(|(kind, _, _)| kind.is_checkout()));

    let at_v1 = source.fork_at(&v1)?;
    assert_eq!(deep_json(&at_v1), json!({"text": "a", "list": []}));
    let diff = source.diff(&v1, &v2)?;
    at_v1.apply_diff(diff)?;
    assert_eq!(
        deep_json(&at_v1),
        json!({"text": "ab", "list": ["item", "nested"]})
    );

    let reverse = source.diff(&v2, &v1)?;
    at_v1.apply_diff(reverse)?;
    assert_eq!(deep_json(&at_v1), json!({"text": "a", "list": []}));

    let snapshot = source.export(ExportMode::Snapshot)?;
    let imported = LoroDoc::new_auto_commit();
    let import_events = Arc::new(Mutex::new(Vec::<(EventTriggerKind, String)>::new()));
    let import_events_ref = Arc::clone(&import_events);
    let _import_sub = imported.subscribe_root(Arc::new(move |event| {
        import_events_ref
            .lock()
            .unwrap()
            .push((event.event_meta.by, event.event_meta.origin.to_string()));
    }));
    imported.import_with(&snapshot, "sync".into())?;
    assert_eq!(deep_json(&imported), deep_json(&source));
    assert_eq!(
        import_events.lock().unwrap().as_slice(),
        &[(EventTriggerKind::Import, "sync".to_string())]
    );

    let batch_target = LoroDoc::new_auto_commit();
    let first_vv = source
        .frontiers_to_vv(&v1)
        .expect("v1 should be in the source oplog");
    let first_updates = source.export(ExportMode::updates(&VersionVector::default()))?;
    let second_updates = source.export(ExportMode::updates(&first_vv))?;
    let status = batch_target.import_batch(&[second_updates, first_updates])?;
    assert!(status.pending.is_none());
    assert_eq!(deep_json(&batch_target), deep_json(&source));

    let handler = imported
        .get_handler(text.id())
        .expect("text handler should exist");
    assert!(matches!(handler, Handler::Text(_)));
    assert!(matches!(handler.to_handler(), Handler::Text(_)));
    assert_eq!(handler.kind(), ContainerType::Text);

    Ok(())
}
