use loro_internal::{delta::DeltaItem, DiffEvent, LoroResult};

#[test]
fn list() -> LoroResult<()> {
    use loro::{LoroDoc, ToJson};
    use serde_json::json;
    let doc = LoroDoc::new();
    check_sync_send(&doc);
    let list = doc.get_list("list");
    list.insert(0, 123)?;
    list.insert(1, 123)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": [123, 123]
        })
    );
    let doc_b = LoroDoc::new();
    doc_b.import(&doc.export_from(&Default::default()))?;
    assert_eq!(
        doc_b.get_deep_value().to_json_value(),
        json!({
            "list": [123, 123]
        })
    );
    let doc_c = LoroDoc::new();
    doc_c.import(&doc.export_snapshot())?;
    assert_eq!(
        doc_c.get_deep_value().to_json_value(),
        json!({
            "list": [123, 123]
        })
    );
    let list = doc_c.get_list("list");
    assert_eq!(list.get_deep_value().to_json_value(), json!([123, 123]));
    Ok(())
}

#[test]
fn map() -> LoroResult<()> {
    use loro::{LoroDoc, LoroValue, ToJson};
    use serde_json::json;
    let doc = LoroDoc::new();
    let map = doc.get_map("map");
    map.insert("key", "value")?;
    map.insert("true", true)?;
    map.insert("null", LoroValue::Null)?;
    map.insert("deleted", LoroValue::Null)?;
    map.delete("deleted")?;
    let text = map
        .insert_container("text", loro_internal::ContainerType::Text)?
        .into_text()
        .unwrap();
    text.insert(0, "Hello world!")?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "map": {
                "key": "value",
                "true": true,
                "null": null,
                "text": "Hello world!"
            }
        })
    );

    Ok(())
}

#[test]
#[cfg(feature = "test_utils")]
fn tree() {
    use loro::{LoroDoc, ToJson};

    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let tree = doc.get_tree("tree");
    let root = tree.create(None).unwrap();
    let root2 = tree.create(None).unwrap();
    tree.mov(root2, root).unwrap();
    let root_meta = tree.get_meta(root).unwrap();
    root_meta.insert("color", "red").unwrap();
    assert_eq!(
        tree.get_value_with_meta().to_json(),
        r#"[{"parent":null,"meta":{"color":"red"},"id":"0@1"},{"parent":"0@1","meta":{},"id":"1@1"}]"#
    )
}

fn check_sync_send(_doc: impl Sync + Send) {}

#[test]
fn richtext_test() {
    use loro::{ExpandType, LoroDoc, ToJson};
    use serde_json::json;

    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "Hello world!").unwrap();
    text.mark(0..5, ExpandType::After, "bold", true).unwrap();
    assert_eq!(
        text.to_delta().to_json_value(),
        json!([
            { "insert": "Hello", "attributes": {"bold": true} },
            { "insert": " world!" },
        ])
    );
    text.unmark(3..5, ExpandType::After, "bold").unwrap();
    assert_eq!(
        text.to_delta().to_json_value(),
        json!([
             { "insert": "Hel", "attributes": {"bold": true} },
             { "insert": "lo", "attributes": {"bold": null} },
             { "insert": " world!" },
        ])
    );
}

#[test]
fn sync() {
    use loro::{LoroDoc, ToJson};
    use serde_json::json;

    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "Hello world!").unwrap();
    let bytes = doc.export_from(&Default::default());
    let doc_b = LoroDoc::new();
    doc_b.import(&bytes).unwrap();
    assert_eq!(doc.get_deep_value(), doc_b.get_deep_value());
    let text_b = doc_b.get_text("text");
    text_b
        .mark(0..5, loro::ExpandType::After, "bold", true)
        .unwrap();
    doc.import(&doc_b.export_from(&doc.oplog_vv())).unwrap();
    assert_eq!(
        text.to_delta().to_json_value(),
        json!([
            { "insert": "Hello", "attributes": {"bold": true} },
            { "insert": " world!" },
        ])
    );
}

#[test]
fn save() {
    use loro::LoroDoc;

    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "123").unwrap();
    let snapshot = doc.export_snapshot();

    let new_doc = LoroDoc::new();
    new_doc.import(&snapshot).unwrap();
    assert_eq!(new_doc.get_deep_value(), doc.get_deep_value());
}

#[test]
fn subscribe() {
    use loro::LoroDoc;
    use std::sync::{atomic::AtomicBool, Arc};

    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    let ran = Arc::new(AtomicBool::new(false));
    let ran2 = ran.clone();

    doc.subscribe(
        &text.id(),
        Arc::new(move |event: DiffEvent| {
            assert!(event.doc.local);
            let event = event.container.diff.as_text().unwrap();
            let delta: Vec<_> = event.iter().cloned().collect();
            let d = DeltaItem::Insert {
                insert: "123".into(),
                attributes: Default::default(),
            };
            assert_eq!(delta, vec![d]);
            ran2.store(true, std::sync::atomic::Ordering::Relaxed);
        }),
    );
    text.insert(0, "123").unwrap();
    doc.commit();
    assert!(ran.load(std::sync::atomic::Ordering::Relaxed));
}
