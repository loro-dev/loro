use std::{cmp::Ordering, sync::Arc};

use loro::{
    awareness::Awareness, FrontiersNotIncluded, LoroDoc, LoroError, LoroList, LoroMap, LoroText,
    ToJson,
};
use loro_internal::{handler::TextDelta, id::ID, vv, LoroResult};
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
    let list = map.insert_container("list", LoroList::new())?; // creating subcontainer will be easier
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
    let mlist = a.get_movable_list("list");
    mlist.get_cursor(5, loro_internal::cursor::Side::Right);
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
