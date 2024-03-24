use std::{cmp::Ordering, sync::Arc};

use loro::{FrontiersNotIncluded, LoroDoc, LoroError, ToJson};
use loro_internal::{handler::TextDelta, id::ID, LoroResult};
use serde_json::json;
use tracing::{trace, trace_span};

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn movable_list() -> Result<(), LoroError> {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    list.insert(0, 1)?;
    list.insert(0, 2)?;
    list.insert(0, 3)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": [3, 2, 1]
        })
    );
    list.mov(0, 2)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": [2, 1, 3]
        })
    );
    list.mov(0, 1)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": [1, 2, 3]
        })
    );

    Ok(())
}

#[test]
fn list_checkout() -> Result<(), LoroError> {
    let doc = LoroDoc::new();
    doc.get_list("list")
        .insert_container(0, loro::ContainerType::Map)?;
    doc.commit();
    let f0 = doc.state_frontiers();
    doc.get_list("list")
        .insert_container(0, loro::ContainerType::Text)?;
    doc.commit();
    let f1 = doc.state_frontiers();
    doc.get_list("list").delete(1, 1)?;
    doc.commit();
    let f2 = doc.state_frontiers();
    doc.get_list("list").delete(0, 1)?;
    doc.commit();
    doc.checkout(&f1)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": ["", {}]
        })
    );
    doc.checkout(&f2)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": [""]
        })
    );
    doc.checkout(&f0)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": [{}]
        })
    );
    doc.checkout(&f1)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "list": ["", {}]
        })
    );
    Ok(())
}

#[test]
fn timestamp() {
    let doc1 = LoroDoc::new();
    doc1.set_peer_id(1).unwrap();
    doc1.get_text("text").insert(0, "0").unwrap();
    doc1.commit();
    doc1.with_oplog(|oplog| {
        let c = oplog.get_change_at(ID::new(1, 0)).unwrap();
        assert!(c.timestamp() == 0);
    });

    doc1.set_record_timestamp(true);
    doc1.get_text("text").insert(0, "0").unwrap();
    doc1.commit();
    let mut last_timestamp = 0;
    doc1.with_oplog(|oplog| {
        let c = oplog.get_change_at(ID::new(1, 1)).unwrap();
        assert!(c.timestamp() > 100000);
        last_timestamp = c.timestamp();
    });

    doc1.get_text("text").insert(0, "0").unwrap();
    doc1.commit();
    doc1.with_oplog(|oplog| {
        let c = oplog.get_change_at(ID::new(1, 2)).unwrap();
        assert!(c.timestamp() < last_timestamp + 10);
    });
}

#[test]
fn cmp_frontiers() {
    let doc1 = LoroDoc::new();
    doc1.set_peer_id(1).unwrap();
    doc1.get_text("text").insert(0, "012345").unwrap();
    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2).unwrap();
    doc2.import(&doc1.export_snapshot()).unwrap();
    doc2.get_text("text").insert(0, "6789").unwrap();
    doc1.import(&doc2.export_snapshot()).unwrap();
    doc1.get_text("text").insert(0, "0123").unwrap();
    doc1.commit();

    assert_eq!(
        doc1.cmp_frontiers(&[].into(), &[ID::new(2, 5)].into()),
        Err(FrontiersNotIncluded)
    );
    assert_eq!(
        doc1.cmp_frontiers(&[ID::new(1, 2)].into(), &[ID::new(2, 3)].into()),
        Ok(Some(Ordering::Less))
    );
    assert_eq!(
        doc1.cmp_frontiers(&[ID::new(1, 5)].into(), &[ID::new(2, 3)].into()),
        Ok(Some(Ordering::Less))
    );
    assert_eq!(
        doc1.cmp_frontiers(&[ID::new(1, 6)].into(), &[ID::new(2, 3)].into()),
        Ok(Some(Ordering::Greater))
    );
    assert_eq!(
        doc1.cmp_frontiers(&[].into(), &[].into()),
        Ok(Some(Ordering::Equal))
    );
    assert_eq!(
        doc1.cmp_frontiers(&[ID::new(1, 6)].into(), &[ID::new(1, 6)].into()),
        Ok(Some(Ordering::Equal))
    );
}

#[test]
fn get_change_at_lamport() {
    let doc1 = LoroDoc::new();
    doc1.set_peer_id(1).unwrap();
    doc1.get_text("text").insert(0, "012345").unwrap();
    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2).unwrap();
    doc2.import(&doc1.export_snapshot()).unwrap();
    doc2.get_text("text").insert(0, "6789").unwrap();
    doc1.import(&doc2.export_snapshot()).unwrap();
    doc1.get_text("text").insert(0, "0123").unwrap();
    doc1.commit();
    doc1.with_oplog(|oplog| {
        let change = oplog.get_change_with_lamport(1, 2).unwrap();
        assert_eq!(change.lamport(), 0);
        assert_eq!(change.peer(), 1);
        let change = oplog.get_change_with_lamport(1, 7).unwrap();
        assert_eq!(change.lamport(), 0);
        assert_eq!(change.peer(), 1);
        let change = oplog.get_change_with_lamport(1, 13).unwrap();
        assert_eq!(change.lamport(), 10);
        assert_eq!(change.peer(), 1);
        let change = oplog.get_change_with_lamport(1, 14).unwrap();
        assert_eq!(change.lamport(), 10);
    })
}

#[test]
fn time_travel() {
    let doc = LoroDoc::new();
    let doc2 = LoroDoc::new();
    let text = doc.get_text("text");
    let text2 = doc2.get_text("text");
    doc.subscribe(
        &text.id(),
        Arc::new(move |x| {
            for event in x.events {
                let Some(delta) = event.diff.as_text() else {
                    continue;
                };
                dbg!(&delta);
                text2.apply_delta(delta).unwrap();
            }
        }),
    );

    let text2 = doc2.get_text("text");
    text.insert(0, "[14497138626449185274] ").unwrap();
    doc.commit();
    text.mark(5..15, "link", true).unwrap();
    doc.commit();
    let f = doc.state_frontiers();
    text.mark(14..20, "bold", true).unwrap();
    doc.commit();
    assert_eq!(text.to_delta(), text2.to_delta());
    doc.checkout(&f).unwrap();
    assert_eq!(text.to_delta(), text2.to_delta());
    doc.attach();
    assert_eq!(text.to_delta(), text2.to_delta());
}

#[test]
fn travel_back_should_remove_styles() {
    let doc = LoroDoc::new();
    let doc2 = LoroDoc::new();
    let text = doc.get_text("text");
    let text2 = doc2.get_text("text");
    doc.subscribe(
        &text.id(),
        Arc::new(move |x| {
            for event in x.events {
                let Some(delta) = event.diff.as_text() else {
                    continue;
                };
                // dbg!(&delta);
                text2.apply_delta(delta).unwrap();
            }
        }),
    );

    let text2 = doc2.get_text("text");
    text.insert(0, "Hello world!").unwrap();
    doc.commit();
    let f = doc.state_frontiers();
    let mut f1 = f.clone();
    f1[0].counter += 1;
    text.mark(0..5, "bold", true).unwrap();
    doc.commit();
    let f2 = doc.state_frontiers();
    assert_eq!(text.to_delta(), text2.to_delta());
    trace_span!("CheckoutToMiddle").in_scope(|| {
        doc.checkout(&f1).unwrap(); // checkout to the middle of the start anchor op and the end anchor op
    });
    doc.checkout(&f).unwrap();
    assert_eq!(
        text.to_delta().as_list().unwrap().len(),
        1,
        "should remove the bold style but got {:?}",
        text.to_delta()
    );
    assert_eq!(doc.state_frontiers(), f);
    doc.check_state_correctness_slow();
    assert_eq!(text.to_delta(), text2.to_delta());
    doc.checkout(&f2).unwrap();
    assert_eq!(text.to_delta(), text2.to_delta());
}

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
    use loro::{LoroDoc, ToJson};
    use serde_json::json;

    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "Hello world!").unwrap();
    text.mark(0..5, "bold", true).unwrap();
    assert_eq!(
        text.to_delta().to_json_value(),
        json!([
            { "insert": "Hello", "attributes": {"bold": true} },
            { "insert": " world!" },
        ])
    );
    text.unmark(3..5, "bold").unwrap();
    assert_eq!(
        text.to_delta().to_json_value(),
        json!([
             { "insert": "Hel", "attributes": {"bold": true} },
             { "insert": "lo world!" },
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
    text_b.mark(0..5, "bold", true).unwrap();
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
        Arc::new(move |event| {
            assert!(event.local);
            for event in event.events {
                let delta = event.diff.as_text().unwrap();
                let d = TextDelta::Insert {
                    insert: "123".into(),
                    attributes: Default::default(),
                };
                assert_eq!(delta, &vec![d]);
                ran2.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }),
    );
    text.insert(0, "123").unwrap();
    doc.commit();
    assert!(ran.load(std::sync::atomic::Ordering::Relaxed));
}
