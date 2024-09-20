use std::{
    cmp::Ordering,
    sync::{atomic::AtomicBool, Arc},
};

use loro::{
    awareness::Awareness, loro_value, Frontiers, FrontiersNotIncluded, LoroDoc, LoroError,
    LoroList, LoroMap, LoroText, ToJson,
};
use loro_internal::{handler::TextDelta, id::ID, vv, LoroResult};
use rand::{Rng, SeedableRng};
use serde_json::json;
use tracing::trace_span;

mod integration_test;

#[ctor::ctor]
fn init() {
    dev_utils::setup_test_log();
}

#[test]
fn insert_an_inserted_movable_handler() -> Result<(), LoroError> {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    list.insert(0, 1)?;
    list.insert(1, 2)?;
    list.insert(2, 3)?;
    list.insert(3, 4)?;
    list.insert(4, 5)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({"list": [1, 2, 3, 4, 5]})
    );
    let list2 = doc.get_list("list2");
    let list3 = list2.insert_container(0, list)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({"list": [1, 2, 3, 4, 5], "list2": [[1, 2, 3, 4, 5]]})
    );
    list3.insert(0, 10)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({"list": [1, 2, 3, 4, 5], "list2": [[10, 1, 2, 3, 4, 5]]})
    );
    Ok(())
}

#[test]
fn fork_doc() -> anyhow::Result<()> {
    let doc0 = LoroDoc::new();
    let text = doc0.get_text("123");
    text.insert(0, "123")?;
    let triggered = Arc::new(AtomicBool::new(false));
    let trigger_cloned = triggered.clone();
    doc0.commit();
    doc0.subscribe_root(Arc::new(move |e| {
        for e in e.events {
            let _t = e.diff.as_text().unwrap();
            triggered.store(true, std::sync::atomic::Ordering::Release);
        }
    }));
    let doc1 = doc0.fork();
    let text1 = doc1.get_text("123");
    assert_eq!(&text1.to_string(), "123");
    text1.insert(3, "456")?;
    assert_eq!(&text.to_string(), "123");
    assert_eq!(&text1.to_string(), "123456");
    assert!(!trigger_cloned.load(std::sync::atomic::Ordering::Acquire),);
    doc0.import(&doc1.export_from(&Default::default()))?;
    assert!(trigger_cloned.load(std::sync::atomic::Ordering::Acquire),);
    assert_eq!(text.to_string(), text1.to_string());
    assert_ne!(doc0.peer_id(), doc1.peer_id());
    Ok(())
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
    doc.get_list("list").insert_container(0, LoroMap::new())?;
    doc.commit();
    let f0 = doc.state_frontiers();
    doc.get_list("list").insert_container(0, LoroText::new())?;
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
        let change = oplog.get_change_with_lamport_lte(1, 2).unwrap();
        assert_eq!(change.lamport(), 0);
        assert_eq!(change.peer(), 1);
        let change = oplog.get_change_with_lamport_lte(1, 7).unwrap();
        assert_eq!(change.lamport(), 0);
        assert_eq!(change.peer(), 1);
        let change = oplog.get_change_with_lamport_lte(1, 13).unwrap();
        assert_eq!(change.lamport(), 10);
        assert_eq!(change.peer(), 1);
        let change = oplog.get_change_with_lamport_lte(1, 14).unwrap();
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
    let text = map.insert_container("text", LoroText::new())?;
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
        r#"[{"parent":null,"meta":{"color":"red"},"id":"0@1","index":0,"fractional_index":"80"},{"parent":"0@1","meta":{},"id":"1@1","index":0,"fractional_index":"80"}]"#
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
            assert!(matches!(
                event.triggered_by,
                loro_internal::event::EventTriggerKind::Local
            ));
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

#[test]
fn prelim_support() -> LoroResult<()> {
    let map = LoroMap::new();
    map.insert("key", "value")?;
    let text = LoroText::new();
    text.insert(0, "123")?;
    let text = map.insert_container("text", text)?;
    let doc = LoroDoc::new();
    let root_map = doc.get_map("map");
    let map = root_map.insert_container("child_map", map)?;
    // `map` is now attached to the doc
    map.insert("1", "223")?; // "223" now presents in the json value of doc
    let list = map.insert_container("list", LoroList::new())?; // creating sub-container will be easier
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "map": {
                "child_map": {
                    "key": "value",
                    "1": "223",
                    "text": "123",
                    "list": []
                }
            }
        })
    );
    assert!(!text.is_attached());
    assert!(list.is_attached());
    text.insert(0, "56")?;
    list.insert(0, 123)?;
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "map": {
                "child_map": {
                    "key": "value",
                    "1": "223",
                    "text": "123",
                    "list": [123]
                }
            }
        })
    );
    Ok(())
}

#[test]
fn decode_import_blob_meta() -> LoroResult<()> {
    let doc_1 = LoroDoc::new();
    doc_1.set_peer_id(1)?;
    doc_1.get_text("text").insert(0, "123")?;
    {
        let bytes = doc_1.export_from(&Default::default());
        let meta = LoroDoc::decode_import_blob_meta(&bytes).unwrap();
        assert!(meta.partial_start_vv.is_empty());
        assert_eq!(meta.partial_end_vv, vv!(1 => 3));
        assert_eq!(meta.start_timestamp, 0);
        assert_eq!(meta.end_timestamp, 0);
        assert!(!meta.is_snapshot);
        assert!(meta.start_frontiers.is_empty());
        assert_eq!(meta.change_num, 1);

        let bytes = doc_1.export_snapshot();
        let meta = LoroDoc::decode_import_blob_meta(&bytes).unwrap();
        assert!(meta.partial_start_vv.is_empty());
        assert_eq!(meta.partial_end_vv, vv!(1 => 3));
        assert_eq!(meta.start_timestamp, 0);
        assert_eq!(meta.end_timestamp, 0);
        assert!(meta.is_snapshot);
        assert!(meta.start_frontiers.is_empty());
        assert_eq!(meta.change_num, 1);
    }

    let doc_2 = LoroDoc::new();
    doc_2.set_peer_id(2)?;
    doc_2.import(&doc_1.export_snapshot()).unwrap();
    doc_2.get_text("text").insert(0, "123")?;
    doc_2.get_text("text").insert(0, "123")?;
    {
        let bytes = doc_2.export_from(&doc_1.oplog_vv());
        let meta = LoroDoc::decode_import_blob_meta(&bytes).unwrap();
        assert_eq!(meta.partial_start_vv, vv!());
        assert_eq!(meta.partial_end_vv, vv!(2 => 6));
        assert_eq!(meta.start_timestamp, 0);
        assert_eq!(meta.end_timestamp, 0);
        assert!(!meta.is_snapshot);
        assert_eq!(meta.start_frontiers, vec![ID::new(1, 2)].into());
        assert_eq!(meta.change_num, 1);

        let bytes = doc_2.export_from(&vv!(1 => 1));
        let meta = LoroDoc::decode_import_blob_meta(&bytes).unwrap();
        assert_eq!(meta.partial_start_vv, vv!(1 => 1));
        assert_eq!(meta.partial_end_vv, vv!(1 => 3, 2 => 6));
        assert_eq!(meta.start_timestamp, 0);
        assert_eq!(meta.end_timestamp, 0);
        assert!(!meta.is_snapshot);
        assert_eq!(meta.start_frontiers, vec![ID::new(1, 0)].into());
        assert_eq!(meta.change_num, 2);

        let bytes = doc_2.export_snapshot();
        let meta = LoroDoc::decode_import_blob_meta(&bytes).unwrap();
        assert_eq!(meta.partial_start_vv, vv!());
        assert_eq!(meta.partial_end_vv, vv!(1 => 3, 2 => 6));
        assert_eq!(meta.start_timestamp, 0);
        assert_eq!(meta.end_timestamp, 0);
        assert!(meta.is_snapshot);
        assert!(meta.start_frontiers.is_empty());
        assert_eq!(meta.change_num, 2);
    }
    Ok(())
}

#[test]
fn init_example() {
    // create meta/users/0/new_user/{name: string, bio: Text}
    let doc = LoroDoc::new();
    let meta = doc.get_map("meta");
    let user = meta
        .get_or_create_container("users", LoroList::new())
        .unwrap()
        .insert_container(0, LoroMap::new())
        .unwrap();
    user.insert("name", "new_user").unwrap();
    user.insert_container("bio", LoroText::new()).unwrap();
}

#[test]
fn get_container_by_str_path() {
    let doc = LoroDoc::new();
    doc.get_map("map")
        .insert_container("key", LoroList::new())
        .unwrap()
        .insert(0, 99)
        .unwrap();
    let c = doc.get_by_str_path("map/key").unwrap();
    assert!(c.as_container().unwrap().is_list());
    c.into_container()
        .unwrap()
        .into_list()
        .unwrap()
        .insert(0, 100)
        .unwrap();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "map": {
                "key": [100, 99]
            }
        })
    );
    let v = doc.get_by_str_path("map/key/1").unwrap();
    assert_eq!(v.into_value().unwrap().into_i64().unwrap(), 99);
    let v = doc.get_by_str_path("map/key/0").unwrap();
    assert_eq!(v.into_value().unwrap().into_i64().unwrap(), 100);

    doc.get_map("map")
        .insert_container("text", LoroText::new())
        .unwrap()
        .insert(0, "123")
        .unwrap();
    let v = doc.get_by_str_path("map/text/0").unwrap();
    assert_eq!(
        v.into_value().unwrap().into_string().unwrap().to_string(),
        "1"
    );
    let v = doc.get_by_str_path("map/text/1").unwrap();
    assert_eq!(
        v.into_value().unwrap().into_string().unwrap().to_string(),
        "2"
    );
    let v = doc.get_by_str_path("map/text/2").unwrap();
    assert_eq!(
        v.into_value().unwrap().into_string().unwrap().to_string(),
        "3"
    );

    let tree = doc.get_tree("tree");
    let node = tree.create(None).unwrap();
    tree.get_meta(node).unwrap().insert("key", "value").unwrap();

    let node_value = doc.get_by_str_path(&format!("tree/{}", node)).unwrap();
    assert!(node_value.into_container().unwrap().is_map());
    let node_map = doc.get_by_str_path(&format!("tree/{}/key", node)).unwrap();
    assert_eq!(
        node_map
            .into_value()
            .unwrap()
            .into_string()
            .unwrap()
            .to_string(),
        "value"
    );
}

#[test]
fn get_cursor() {
    let doc1 = LoroDoc::new();
    doc1.set_peer_id(1).unwrap();
    let text = doc1.get_text("text");
    text.insert(0, "6789").unwrap();
    let pos_7 = text.get_cursor(1, Default::default()).unwrap();
    let pos_info = doc1.get_cursor_pos(&pos_7).unwrap();
    assert!(pos_info.update.is_none());
    assert_eq!(pos_info.current.pos, 1);
    text.insert(0, "012345").unwrap();
    let pos_info = doc1.get_cursor_pos(&pos_7).unwrap();
    assert!(pos_info.update.is_none());
    assert_eq!(pos_info.current.pos, 7);

    // test merge
    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2).unwrap();
    let text2 = doc2.get_text("text");
    text2.insert(0, "ab").unwrap();
    let pos_a = text2.get_cursor(0, Default::default()).unwrap();
    let pos_info = doc2.get_cursor_pos(&pos_a).unwrap();
    assert!(pos_info.update.is_none());
    assert_eq!(pos_info.current.pos, 0);
    // text2: 0123456789ab
    doc2.import(&doc1.export_snapshot()).unwrap();
    let pos_info = doc2.get_cursor_pos(&pos_a).unwrap();
    assert!(pos_info.update.is_none());
    assert_eq!(pos_info.current.pos, 10);

    // test delete
    // text2: 01234~~56789~~ab
    // text2: 01234ab
    //            |___ pos_7
    text2.delete(5, 5).unwrap(); // pos_7 now is 5
    let pos_info = doc2.get_cursor_pos(&pos_7).unwrap(); // it should be fine to query from another doc
    assert_eq!(pos_info.update.as_ref().unwrap().id.unwrap(), ID::new(2, 0));
    assert_eq!(pos_info.current.pos, 5);

    // rich text
    //
    // text2: [01]234ab
    //              |___ pos_7
    text2.mark(0..2, "bold", true).unwrap();
    let pos_info = doc2.get_cursor_pos(&pos_7).unwrap();
    assert_eq!(pos_info.update.as_ref().unwrap().id.unwrap(), ID::new(2, 0));
    assert_eq!(pos_info.current.pos, 5); // should not be affected by rich text mark
}

#[test]
fn get_cursor_at_the_end() {
    let doc = LoroDoc::new();
    let text = &doc.get_text("text");
    text.insert(0, "01234").unwrap();
    let pos = text.get_cursor(5, Default::default()).unwrap();
    assert_eq!(doc.get_cursor_pos(&pos).unwrap().current.pos, 5);
    text.insert(0, "01234").unwrap();
    assert_eq!(doc.get_cursor_pos(&pos).unwrap().current.pos, 10);
    text.delete(0, 10).unwrap();
    assert_eq!(doc.get_cursor_pos(&pos).unwrap().current.pos, 0);
    text.insert(0, "01234").unwrap();
    assert_eq!(doc.get_cursor_pos(&pos).unwrap().current.pos, 5);
}

#[test]
fn get_cursor_for_list() {
    let doc = LoroDoc::new();
    let list = doc.get_list("list");
    let pos_start = list.get_cursor(0, Default::default()).unwrap();
    list.insert(0, 1).unwrap();
    let pos_0 = list.get_cursor(0, Default::default()).unwrap();
    let pos_end = list.get_cursor(1, Default::default()).unwrap();
    {
        let result = doc.get_cursor_pos(&pos_start).unwrap();
        assert_eq!(result.current.pos, 0);
    }
    {
        let result = doc.get_cursor_pos(&pos_0).unwrap();
        assert_eq!(result.current.pos, 0);
    }
    {
        let result = doc.get_cursor_pos(&pos_end).unwrap();
        assert_eq!(result.current.pos, 1);
    }
    list.insert(0, 1).unwrap();
    {
        let result = doc.get_cursor_pos(&pos_start).unwrap();
        assert_eq!(result.current.pos, 0);
    }
    {
        let result = doc.get_cursor_pos(&pos_0).unwrap();
        assert_eq!(result.current.pos, 1);
    }
    {
        let result = doc.get_cursor_pos(&pos_end).unwrap();
        assert_eq!(result.current.pos, 2);
    }
    list.insert(0, 1).unwrap();
    {
        let result = doc.get_cursor_pos(&pos_start).unwrap();
        assert_eq!(result.current.pos, 0);
    }
    {
        let result = doc.get_cursor_pos(&pos_0).unwrap();
        assert_eq!(result.current.pos, 2);
    }
    {
        let result = doc.get_cursor_pos(&pos_end).unwrap();
        assert_eq!(result.current.pos, 3);
    }
    list.insert(0, 1).unwrap();
    {
        let result = doc.get_cursor_pos(&pos_start).unwrap();
        assert_eq!(result.current.pos, 0);
    }
    {
        let result = doc.get_cursor_pos(&pos_0).unwrap();
        assert_eq!(result.current.pos, 3);
    }
    {
        let result = doc.get_cursor_pos(&pos_end).unwrap();
        assert_eq!(result.current.pos, 4);
    }
}

#[test]
fn get_out_of_bound_cursor() {
    let a = LoroDoc::new();
    let text = a.get_text("text");
    text.insert(0, "123").unwrap();
    text.get_cursor(5, loro_internal::cursor::Side::Right);
    let list = a.get_list("list");
    list.get_cursor(5, loro_internal::cursor::Side::Right);
    let m_list = a.get_movable_list("list");
    m_list.get_cursor(5, loro_internal::cursor::Side::Right);
}

#[test]
fn awareness() {
    let mut a = Awareness::new(1, 1);
    a.set_local_state(1);
    assert_eq!(a.get_local_state(), Some(1.into()));
    a.set_local_state(2);
    assert_eq!(a.get_local_state(), Some(2.into()));

    let mut b = Awareness::new(2, 1);
    let (updated, added) = b.apply(&a.encode_all());
    assert_eq!(updated.len(), 0);
    assert_eq!(added, vec![1]);
    assert_eq!(
        b.get_all_states().get(&1).map(|x| x.state.clone()),
        Some(2.into())
    );
    assert_eq!(b.get_all_states().get(&2).map(|x| x.state.clone()), None);
}

#[test]
// https://github.com/loro-dev/loro/issues/397
fn len_and_is_empty_inconsistency() {
    let doc = LoroDoc::new();
    let map = doc.get_map("map");
    println!("{:#?}", map);
    assert!(map.is_empty());
    map.insert("leaf", 42i64).unwrap();
    println!("{:#?}", map.get("leaf"));

    assert_eq!(map.len(), 1);
    map.delete("leaf").unwrap();
    println!("{:#?}", map.get("leaf"));
    assert_eq!(map.len(), 0);
    assert!(map.is_empty());
}

#[test]
fn fast_snapshot_for_updates() {
    let doc_a = LoroDoc::new();
    // Create some random edits on doc_a
    let text = doc_a.get_text("text");
    text.insert(0, "Hello, world!").unwrap();

    let list = doc_a.get_list("list");
    list.insert(0, 42).unwrap();
    list.insert(1, "foo").unwrap();

    let map = doc_a.get_map("map");
    map.insert("key1", "value1").unwrap();
    map.insert("key2", 3.).unwrap();

    doc_a.commit();

    // Create doc_b
    let doc_b = LoroDoc::new();

    // Create some random edits on doc_b
    let text_b = doc_b.get_text("text_b");
    text_b.insert(0, "Greetings!").unwrap();

    let list_b = doc_b.get_list("list_b");
    list_b.insert(0, "bar").unwrap();
    list_b.insert(1, 99).unwrap();

    let map_b = doc_b.get_map("map_b");
    map_b.insert("keyA", true).unwrap();
    map_b.insert("keyB", loro_value!([1, 2, 3])).unwrap();

    doc_b.commit();

    doc_b
        .import(&doc_a.export(loro::ExportMode::Snapshot))
        .unwrap();
    doc_a
        .import(&doc_b.export(loro::ExportMode::Snapshot))
        .unwrap();

    assert_eq!(doc_a.get_deep_value(), doc_b.get_deep_value());
}

#[test]
fn new_update_encode_mode() {
    let doc = LoroDoc::new();
    // Create some random edits on doc
    let text = doc.get_text("text");
    text.insert(0, "Hello, world!").unwrap();

    let list = doc.get_list("list");
    list.insert(0, 42).unwrap();
    list.insert(1, "foo").unwrap();

    let map = doc.get_map("map");
    map.insert("key1", "value1").unwrap();
    map.insert("key2", 3).unwrap();

    doc.commit();

    // Create another doc
    let doc2 = LoroDoc::new();

    // Export updates from doc and import to doc2
    let updates = doc.export(loro::ExportMode::all_updates());
    doc2.import(&updates).unwrap();

    // Check equality
    assert_eq!(doc.get_deep_value(), doc2.get_deep_value());
    // Make some edits on doc2
    let text2 = doc2.get_text("text");
    text2.insert(13, " How are you?").unwrap();

    let list2 = doc2.get_list("list");
    list2.insert(2, "bar").unwrap();

    let map2 = doc2.get_map("map");
    map2.insert("key3", 4.5).unwrap();

    doc2.commit();

    // Export updates from doc2 and import to doc
    let updates2 = doc2.export(loro::ExportMode::updates(&doc.oplog_vv()));
    doc.import(&updates2).unwrap();

    // Check equality after syncing back
    assert_eq!(doc.get_deep_value(), doc2.get_deep_value());
}

fn apply_random_ops(doc: &LoroDoc, seed: u64, mut op_len: usize) {
    let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
    while op_len > 0 {
        match rng.gen_range(0..6) {
            0 => {
                // Insert text
                let text = doc.get_text("text");
                let pos = rng.gen_range(0..=text.len_unicode());
                let content = rng.gen_range('A'..='z').to_string();
                text.insert(pos, &content).unwrap();
                op_len -= 1;
            }
            1 => {
                // Delete text
                let text = doc.get_text("text");
                if text.len_unicode() > 0 {
                    let start = rng.gen_range(0..text.len_unicode());
                    text.delete(start, 1).unwrap();
                    op_len -= 1;
                }
            }
            2 => {
                // Insert into map
                let map = doc.get_map("map");
                let key = format!("key{}", rng.gen::<u32>());
                let value = rng.gen::<i32>();
                map.insert(&key, value).unwrap();
                op_len -= 1;
            }
            3 => {
                // Push to list
                let list = doc.get_list("list");
                let item = format!("item{}", rng.gen::<u32>());
                list.push(item).unwrap();
                op_len -= 1;
            }
            4 => {
                // Create node in tree
                let tree = doc.get_tree("tree");
                tree.create(None).unwrap();
                op_len -= 1;
            }
            5 => {
                // Push to movable list
                let list = doc.get_movable_list("movable_list");
                let item = format!("item{}", rng.gen::<u32>());
                list.push(item).unwrap();
                op_len -= 1;
            }
            _ => unreachable!(),
        }
    }

    doc.commit();
}

#[test]
fn test_gc_sync() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    apply_random_ops(&doc, 123, 11);
    let bytes = doc.export(loro::ExportMode::gc_snapshot_from_id(
        ID::new(doc.peer_id(), 10).into(),
    ));

    let new_doc = LoroDoc::new();
    new_doc.set_peer_id(2).unwrap();
    new_doc.import(&bytes).unwrap();
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());
    let trim_end = new_doc.trimmed_vv().get(&doc.peer_id()).copied().unwrap();
    assert_eq!(trim_end, 10);

    apply_random_ops(&new_doc, 1234, 5);
    let updates = new_doc.export(loro::ExportMode::updates_owned(doc.oplog_vv()));
    doc.import(&updates).unwrap();
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());

    apply_random_ops(&doc, 11, 5);
    let updates = doc.export(loro::ExportMode::updates_owned(new_doc.oplog_vv()));
    new_doc.import(&updates).unwrap();
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());
}

#[test]
fn test_gc_empty() {
    let doc = LoroDoc::new();
    apply_random_ops(&doc, 123, 11);
    let bytes = doc.export(loro::ExportMode::gc_snapshot(&Frontiers::default()));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes).unwrap();
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());
    apply_random_ops(&new_doc, 0, 10);
    doc.import(&new_doc.export_from(&Default::default()))
        .unwrap();
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());

    let bytes = new_doc.export(loro::ExportMode::Snapshot);
    let doc_c = LoroDoc::new();
    doc_c.import(&bytes).unwrap();
    assert_eq!(doc_c.get_deep_value(), new_doc.get_deep_value());
}

#[test]
fn test_gc_import_outdated_updates() {
    let doc = LoroDoc::new();
    apply_random_ops(&doc, 123, 11);
    let bytes = doc.export(loro::ExportMode::gc_snapshot_from_id(
        ID::new(doc.peer_id(), 5).into(),
    ));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes).unwrap();

    let other_doc = LoroDoc::new();
    apply_random_ops(&other_doc, 123, 11);
    let err = new_doc
        .import(&other_doc.export_from(&Default::default()))
        .unwrap_err();
    assert_eq!(err, LoroError::ImportUpdatesThatDependsOnOutdatedVersion);
}

#[test]
fn test_gc_import_pending_updates_that_is_outdated() {
    let doc = LoroDoc::new();
    apply_random_ops(&doc, 123, 11);
    let bytes = doc.export(loro::ExportMode::gc_snapshot_from_id(
        ID::new(doc.peer_id(), 5).into(),
    ));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes).unwrap();

    let other_doc = LoroDoc::new();
    apply_random_ops(&other_doc, 123, 5);
    let bytes_a = other_doc.export_from(&Default::default());
    let vv = other_doc.oplog_vv();
    apply_random_ops(&other_doc, 123, 5);
    let bytes_b = other_doc.export_from(&vv);
    // pending
    new_doc.import(&bytes_b).unwrap();
    let err = new_doc.import(&bytes_a).unwrap_err();
    assert_eq!(err, LoroError::ImportUpdatesThatDependsOnOutdatedVersion);
}

#[test]
fn test_calling_exporting_snapshot_on_gc_doc() {
    let doc = LoroDoc::new();
    apply_random_ops(&doc, 123, 11);
    let bytes = doc.export(loro::ExportMode::gc_snapshot_from_id(
        ID::new(doc.peer_id(), 5).into(),
    ));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes).unwrap();
    let snapshot = new_doc.export(loro::ExportMode::Snapshot);
    let doc_c = LoroDoc::new();
    doc_c.import(&snapshot).unwrap();
    assert_eq!(doc_c.get_deep_value(), new_doc.get_deep_value());
    assert_eq!(new_doc.trimmed_vv(), doc_c.trimmed_vv());
}

#[test]
fn sync_two_trimmed_docs() {
    let doc = LoroDoc::new();
    apply_random_ops(&doc, 123, 11);
    let bytes = doc.export(loro::ExportMode::gc_snapshot_from_id(
        ID::new(doc.peer_id(), 10).into(),
    ));

    let doc_a = LoroDoc::new();
    doc_a.import(&bytes).unwrap();
    let doc_b = LoroDoc::new();
    doc_b.import(&bytes).unwrap();
    apply_random_ops(&doc_a, 12312, 10);
    apply_random_ops(&doc_b, 2312, 10);

    // Sync doc_a and doc_b
    let bytes_a = doc_a.export_from(&doc_b.oplog_vv());
    let bytes_b = doc_b.export_from(&doc_a.oplog_vv());

    doc_a.import(&bytes_b).unwrap();
    doc_b.import(&bytes_a).unwrap();

    // Check if doc_a and doc_b are equal after syncing
    assert_eq!(doc_a.get_deep_value(), doc_b.get_deep_value());
    assert_eq!(doc_a.oplog_vv(), doc_b.oplog_vv());
    assert_eq!(doc_a.oplog_frontiers(), doc_b.oplog_frontiers());
    assert_eq!(doc_a.state_vv(), doc_b.state_vv());
    assert_eq!(doc_a.trimmed_vv(), doc_b.trimmed_vv());
}

#[test]
fn test_map_checkout_on_trimmed_doc() {
    let doc = LoroDoc::new();
    doc.get_map("map").insert("0", 0).unwrap();
    doc.get_map("map").insert("1", 1).unwrap();
    doc.get_map("map").insert("2", 2).unwrap();
    doc.get_map("map").insert("3", 3).unwrap();
    doc.get_map("map").insert("2", 4).unwrap();

    let new_doc_bytes = doc.export(loro::ExportMode::gc_snapshot_from_id(
        ID::new(doc.peer_id(), 1).into(),
    ));

    let new_doc = LoroDoc::new();
    new_doc.import(&new_doc_bytes).unwrap();
    assert_eq!(
        new_doc.get_deep_value(),
        loro_value!({
            "map": {
                "0": 0,
                "1": 1,
                "2": 4,
                "3": 3,
            }
        })
    );
    new_doc.checkout(&ID::new(doc.peer_id(), 2).into()).unwrap();
    assert_eq!(
        new_doc.get_deep_value(),
        loro_value!({
            "map": {
                "0": 0,
                "1": 1,
                "2": 2,
            }
        })
    );
    new_doc.checkout(&ID::new(doc.peer_id(), 1).into()).unwrap();
    assert_eq!(
        new_doc.get_deep_value(),
        loro_value!({
            "map": {
                "0": 0,
                "1": 1,
            }
        })
    );
    new_doc.checkout_to_latest();
    assert_eq!(
        new_doc.get_deep_value(),
        loro_value!({
            "map": {
                "0": 0,
                "1": 1,
                "2": 4,
                "3": 3,
            }
        })
    );

    let err = new_doc
        .checkout(&ID::new(doc.peer_id(), 0).into())
        .unwrap_err();
    assert_eq!(err, LoroError::SwitchToTrimmedVersion);
}

#[test]
fn test_loro_export_local_updates() {
    use std::sync::{Arc, Mutex};

    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    let updates = Arc::new(Mutex::new(Vec::new()));

    let updates_clone = updates.clone();
    let subscription = doc.subscribe_local_update(Box::new(move |bytes: &[u8]| {
        updates_clone.lock().unwrap().push(bytes.to_vec());
    }));

    // Make some changes
    text.insert(0, "Hello").unwrap();
    doc.commit();
    text.insert(5, " world").unwrap();
    doc.commit();

    // Check that updates were recorded
    {
        let recorded_updates = updates.lock().unwrap();
        assert_eq!(recorded_updates.len(), 2);

        // Verify the content of the updates
        let doc_b = LoroDoc::new();
        doc_b.import(&recorded_updates[0]).unwrap();
        assert_eq!(doc_b.get_text("text").to_string(), "Hello");

        doc_b.import(&recorded_updates[1]).unwrap();
        assert_eq!(doc_b.get_text("text").to_string(), "Hello world");
    }

    {
        // Test that the subscription can be dropped
        drop(subscription);
        // Make another change
        text.insert(11, "!").unwrap();
        doc.commit();
        // Check that no new update was recorded
        assert_eq!(updates.lock().unwrap().len(), 2);
    }
}

fn test_movable_list_checkout_on_trimmed_doc() -> LoroResult<()> {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    list.insert(0, 0)?;
    list.set(0, 1)?;
    list.set(0, 3)?;
    list.insert(1, 2)?;
    list.mov(1, 0)?;
    list.delete(0, 1)?;
    list.set(0, 0)?;
    let new_doc_bytes = doc.export(loro::ExportMode::gc_snapshot_from_id(
        ID::new(doc.peer_id(), 2).into(),
    ));

    let new_doc = LoroDoc::new();
    new_doc.import(&new_doc_bytes).unwrap();
    assert_eq!(
        new_doc.get_deep_value(),
        loro_value!({
            "list": [0]
        })
    );
    new_doc.checkout(&ID::new(doc.peer_id(), 2).into()).unwrap();
    assert_eq!(
        new_doc.get_deep_value(),
        loro_value!({
            "list": [3]
        })
    );

    new_doc.checkout_to_latest();
    assert_eq!(
        new_doc.get_deep_value(),
        loro_value!({
            "list": [0]
        })
    );

    let err = new_doc
        .checkout(&ID::new(doc.peer_id(), 1).into())
        .unwrap_err();
    assert_eq!(err, LoroError::SwitchToTrimmedVersion);
    Ok(())
}

#[test]
fn test_tree_checkout_on_trimmed_doc() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(0)?;
    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let root = tree.create(None)?;
    let child1 = tree.create(None)?;
    tree.mov(child1, root)?;
    let child2 = tree.create(None).unwrap();
    tree.mov(child2, root)?;

    let new_doc_bytes = doc.export(loro::ExportMode::gc_snapshot_from_id(
        ID::new(doc.peer_id(), 1).into(),
    ));

    let new_doc = LoroDoc::new();
    new_doc.import(&new_doc_bytes).unwrap();
    assert_eq!(
        new_doc.get_deep_value(),
        loro_value!({
            "tree": [
                {
                    "parent": null,
                    "meta":{},
                    "id": "0@0",
                    "index": 0,
                    "fractional_index": "80",
                },
                {
                    "parent": "0@0",
                    "meta":{},
                    "id": "1@0",
                    "index": 0,
                    "fractional_index": "80",
                },
                {
                    "parent": "0@0",
                    "meta":{},
                    "id": "3@0",
                    "index": 1,
                    "fractional_index": "8180",
                },
            ]
        })
    );
    new_doc.checkout(&ID::new(doc.peer_id(), 2).into()).unwrap();
    assert_eq!(
        new_doc.get_deep_value(),
        loro_value!({
            "tree": [
                {
                    "parent": null,
                    "meta":{},
                    "id": "0@0",
                    "index": 0,
                    "fractional_index": "80",
                },
                {
                    "parent": "0@0",
                    "meta":{},
                    "id": "1@0",
                    "index": 0,
                    "fractional_index": "80",
                },
            ]
        })
    );
    new_doc.checkout(&ID::new(doc.peer_id(), 1).into()).unwrap();
    assert_eq!(
        new_doc.get_deep_value(),
        loro_value!({
            "tree": [
                {
                    "parent": null,
                    "meta":{},
                    "id": "0@0",
                    "index": 0,
                    "fractional_index": "80",
                },
                {
                    "parent": null,
                    "meta":{},
                    "id": "1@0",
                    "index": 1,
                    "fractional_index": "8180",
                },
            ]
        })
    );
    new_doc.checkout_to_latest();
    assert_eq!(
        new_doc.get_deep_value(),
        loro_value!({
            "tree": [
                {
                    "parent": null,
                    "meta":{},
                    "id": "0@0",
                    "index": 0,
                    "fractional_index": "80",
                },
                {
                    "parent": "0@0",
                    "meta":{},
                    "id": "1@0",
                    "index": 0,
                    "fractional_index": "80",
                },
                {
                    "parent": "0@0",
                    "meta":{},
                    "id": "3@0",
                    "index": 1,
                    "fractional_index": "8180",
                },
            ]
        })
    );

    let err = new_doc
        .checkout(&ID::new(doc.peer_id(), 0).into())
        .unwrap_err();
    assert_eq!(err, LoroError::SwitchToTrimmedVersion);
    Ok(())
}

#[test]
fn test_tree_with_other_ops_checkout_on_trimmed_doc() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(0)?;
    let tree = doc.get_tree("tree");
    let root = tree.create(None)?;
    tree.enable_fractional_index(0);
    let child1 = tree.create(None)?;
    tree.mov(child1, root)?;
    let child2 = tree.create(None).unwrap();
    tree.mov(child2, root)?;
    let map = doc.get_map("map");
    map.insert("0", 0)?;
    map.insert("1", 1)?;
    doc.commit();
    let gc_frontiers = doc.oplog_frontiers();
    map.insert("2", 2)?;
    tree.mov(child2, child1)?;
    tree.delete(child1)?;

    let new_doc_bytes = doc.export(loro::ExportMode::gc_snapshot(&gc_frontiers));

    let new_doc = LoroDoc::new();
    new_doc.import(&new_doc_bytes).unwrap();

    new_doc.checkout(&gc_frontiers)?;
    let value = new_doc.get_deep_value();
    assert_eq!(
        value,
        loro_value!(
            {
                "map":{
                    "0":0,
                    "1":1,
                },
                "tree":[
                {
                    "parent": null,
                    "meta":{},
                    "id": "0@0",
                    "index": 0,
                    "fractional_index": "80",
                },
                {
                    "parent": "0@0",
                    "meta":{},
                    "id": "1@0",
                    "index": 0,
                    "fractional_index": "80",
                },
                {
                    "parent": "0@0",
                    "meta":{},
                    "id": "3@0",
                    "index": 1,
                    "fractional_index": "8180",
                },
            ]
            }
        )
    );

    let err = new_doc
        .checkout(&ID::new(doc.peer_id(), 0).into())
        .unwrap_err();
    assert_eq!(err, LoroError::SwitchToTrimmedVersion);
    Ok(())
}

#[test]
fn test_gc_can_remove_unreachable_states() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    let map = doc.get_map("map");
    map.insert("1", 1)?; // 0
    let list = map.insert_container("0", LoroList::new())?; // 1
    list.insert_container(0, LoroText::new())?; // 2
    list.insert_container(1, LoroText::new())?; // 3
                                                // {
                                                //     "map": {
                                                //         "0": [{
                                                //             "text": ""
                                                //         }, {
                                                //             "text": ""
                                                //         }],
                                                //         "1", "1"
                                                //     }
                                                // }
    doc.commit();

    {
        assert_eq!(doc.analyze().dropped_len(), 0);
        map.insert("0", 0)?; // 4
                             // {
                             //     "map": {
                             //         "0": 0,
                             //         "1", 1
                             //     }
                             // }
        doc.commit();
        assert_eq!(doc.analyze().dropped_len(), 3);
    }

    doc.checkout(&Frontiers::from(ID::new(1, 3))).unwrap();
    assert_eq!(doc.analyze().len(), 4);
    assert_eq!(doc.analyze().dropped_len(), 0);
    doc.checkout_to_latest();

    {
        let snapshot = doc.export(loro::ExportMode::gc_snapshot_from_id(ID::new(1, 3)));
        let new_doc = LoroDoc::new();
        new_doc.import(&snapshot)?;
        let a = new_doc.analyze();
        assert_eq!(a.len(), 4);
        assert_eq!(a.dropped_len(), 3);
        new_doc.checkout(&Frontiers::from(ID::new(1, 3))).unwrap();
        let a = new_doc.analyze();
        assert_eq!(a.len(), 4);
        assert_eq!(a.dropped_len(), 0);
    }

    {
        let snapshot = doc.export(loro::ExportMode::gc_snapshot_from_id(ID::new(1, 4)));
        let new_doc = LoroDoc::new();
        new_doc.import(&snapshot)?;
        assert_eq!(new_doc.analyze().dropped_len(), 0);
    }

    Ok(())
}

#[test]
fn small_update_size() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "h").unwrap();
    let bytes = doc.export(loro::ExportMode::all_updates());
    println!("Update bytes {:?}", dev_utils::ByteSize(bytes.len()));
    assert!(bytes.len() < 90, "Large update size {}", bytes.len());
}

#[test]
fn test_tree_move() {
    let doc = LoroDoc::new();
    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let root1 = tree.create(None).unwrap();
    let node1 = tree.create(root1).unwrap();
    let node2 = tree.create(root1).unwrap();
    assert_eq!(tree.children(Some(root1)).unwrap(), vec![node1, node2]);
    tree.mov_before(node2, node1).unwrap();
    assert_eq!(tree.children(Some(root1)).unwrap(), vec![node2, node1]);
    tree.mov_before(node2, node1).unwrap();
    assert_eq!(tree.children(Some(root1)).unwrap(), vec![node2, node1]);

    tree.mov_after(node2, node1).unwrap();
    assert_eq!(tree.children(Some(root1)).unwrap(), vec![node1, node2]);
    tree.mov_after(node2, node1).unwrap();
    assert_eq!(tree.children(Some(root1)).unwrap(), vec![node1, node2]);
}

#[test]
fn richtext_map_value() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "Hello").unwrap();
    text.mark(0..2, "comment", loro_value!({"b": {}})).unwrap();
    let delta = text.to_delta();
    assert_eq!(
        delta,
        loro_value!([
            {
                "insert": "He",
                "attributes": {
                    "comment": {
                        "b": {}
                    }
                }
            },
            {
                "insert": "llo",
            }
        ])
    );
}

#[test]
fn test_get_shallow_value() {
    let doc = LoroDoc::new();
    let _tree = doc.get_tree("tree");
    let _list = doc.get_list("list");
    let _map = doc.get_map("map");
    let _text = doc.get_text("text");
    let _movable_list = doc.get_movable_list("movable_list");
    let v = doc.get_value();
    let v = v.as_map().unwrap();
    assert!(v.contains_key("tree"));
    assert!(v.contains_key("list"));
    assert!(v.contains_key("map"));
    assert!(v.contains_key("text"));
    assert!(v.contains_key("movable_list"));
}

#[test]
fn perform_action_on_deleted_container_should_return_error() {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    let text = list.push_container(LoroText::new()).unwrap();
    list.set(0, 1).unwrap();
    let result = text.insert(0, "Hello");
    match result {
        Ok(_) => panic!("Expected error, but operation succeeded"),
        Err(LoroError::ContainerDeleted { .. }) => {}
        _ => panic!("Expected ContainerDeleted error, but got something else"),
    }
    assert!(text.is_deleted());
}

#[test]
fn checkout_should_reset_container_deleted_cache() {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    let text = list.push_container(LoroText::new()).unwrap();
    doc.commit();
    let f = doc.state_frontiers();
    list.set(0, 1).unwrap();
    assert!(text.is_deleted());
    doc.checkout(&f).unwrap();
    assert!(!text.is_deleted());
}

#[test]
fn test_fork_at_target_frontiers() {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    let _text = list.push_container(LoroText::new()).unwrap();
    doc.commit();
    let f = doc.state_frontiers();
    list.set(0, 1).unwrap();
    doc.commit();
    let snapshot = doc.export(loro::ExportMode::snapshot_at(&f));
    let new_doc = LoroDoc::new();
    new_doc.import(&snapshot).unwrap();
    assert_eq!(new_doc.state_frontiers(), f);
    assert_eq!(
        new_doc.get_deep_value().to_json_value(),
        json!({
            "list": [""]
        })
    );
    new_doc
        .import(&doc.export(loro::ExportMode::all_updates()))
        .unwrap();
    assert_eq!(
        new_doc.get_deep_value().to_json_value(),
        json!({
            "list": [1]
        })
    );
}
