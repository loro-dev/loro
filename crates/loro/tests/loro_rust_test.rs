#![allow(deprecated)]
#![allow(unexpected_cfgs)]
use dev_utils::ByteSize;
use pretty_assertions::assert_eq;
use std::{
    cmp::Ordering,
    collections::HashSet,
    ops::ControlFlow,
    sync::{
        atomic::{AtomicBool, AtomicU64},
        Arc,
    },
};

use loro::{
    awareness::Awareness,
    event::{Diff, DiffBatch, ListDiffItem},
    loro_value, CommitOptions, ContainerID, ContainerTrait, ContainerType, ExportMode, Frontiers,
    FrontiersNotIncluded, IdSpan, Index, LoroDoc, LoroError, LoroList, LoroMap, LoroMapValue,
    LoroMovableList, LoroStringValue, LoroText, LoroTree, LoroValue, ToJson, TreeParentId,
};
use loro_internal::{
    encoding::EncodedBlobMode, fx_map, handler::TextDelta, id::ID, version_range, vv, LoroResult,
};
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
    let _g = doc0.subscribe_root(Arc::new(move |e| {
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
    let _g = doc.subscribe(
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
    let _g = doc.subscribe(
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
    let mut f1 = f.to_vec();
    f1[0].counter += 1;
    text.mark(0..5, "bold", true).unwrap();
    doc.commit();
    let f2 = doc.state_frontiers();
    assert_eq!(text.to_delta(), text2.to_delta());
    trace_span!("CheckoutToMiddle").in_scope(|| {
        doc.checkout(&f1.into()).unwrap(); // checkout to the middle of the start anchor op and the end anchor op
    });
    doc.checkout(&f).unwrap();
    assert_eq!(
        text.get_richtext_value().as_list().unwrap().len(),
        1,
        "should remove the bold style but got {:?}",
        text.get_richtext_value()
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
        r#"[{"parent":null,"meta":{"color":"red"},"id":"0@1","index":0,"children":[{"parent":"0@1","meta":{},"id":"1@1","index":0,"children":[],"fractional_index":"80"}],"fractional_index":"80"}]"#
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
        text.get_richtext_value().to_json_value(),
        json!([
            { "insert": "Hello", "attributes": {"bold": true} },
            { "insert": " world!" },
        ])
    );
    text.unmark(3..5, "bold").unwrap();
    assert_eq!(
        text.get_richtext_value().to_json_value(),
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
        text.get_richtext_value().to_json_value(),
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

    let _g = doc.subscribe(
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
        let meta = LoroDoc::decode_import_blob_meta(&bytes, false).unwrap();
        assert!(meta.partial_start_vv.is_empty());
        assert_eq!(meta.partial_end_vv, vv!(1 => 3));
        assert_eq!(meta.start_timestamp, 0);
        assert_eq!(meta.end_timestamp, 0);
        assert!(!meta.mode.is_snapshot());
        assert!(meta.start_frontiers.is_empty());
        assert_eq!(meta.change_num, 1);

        let bytes = doc_1.export_snapshot();
        let meta = LoroDoc::decode_import_blob_meta(&bytes, false).unwrap();
        assert!(meta.partial_start_vv.is_empty());
        assert_eq!(meta.partial_end_vv, vv!(1 => 3));
        assert_eq!(meta.start_timestamp, 0);
        assert_eq!(meta.end_timestamp, 0);
        assert!(meta.mode.is_snapshot());
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
        let meta = LoroDoc::decode_import_blob_meta(&bytes, false).unwrap();
        assert_eq!(meta.partial_start_vv, vv!());
        assert_eq!(meta.partial_end_vv, vv!(2 => 6));
        assert_eq!(meta.start_timestamp, 0);
        assert_eq!(meta.end_timestamp, 0);
        assert!(!meta.mode.is_snapshot());
        assert_eq!(meta.start_frontiers, vec![ID::new(1, 2)].into());
        assert_eq!(meta.change_num, 1);

        let bytes = doc_2.export_from(&vv!(1 => 1));
        let meta = LoroDoc::decode_import_blob_meta(&bytes, false).unwrap();
        assert_eq!(meta.partial_start_vv, vv!(1 => 1));
        assert_eq!(meta.partial_end_vv, vv!(1 => 3, 2 => 6));
        assert_eq!(meta.start_timestamp, 0);
        assert_eq!(meta.end_timestamp, 0);
        assert!(!meta.mode.is_snapshot());
        assert_eq!(meta.start_frontiers, vec![ID::new(1, 0)].into());
        assert_eq!(meta.change_num, 2);

        let bytes = doc_2.export_snapshot();
        let meta = LoroDoc::decode_import_blob_meta(&bytes, false).unwrap();
        assert_eq!(meta.partial_start_vv, vv!());
        assert_eq!(meta.partial_end_vv, vv!(1 => 3, 2 => 6));
        assert_eq!(meta.start_timestamp, 0);
        assert_eq!(meta.end_timestamp, 0);
        assert!(meta.mode.is_snapshot());
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
    let mut a = Awareness::new(1, 1000);
    a.set_local_state(1);
    assert_eq!(a.get_local_state(), Some(1.into()));
    a.set_local_state(2);
    assert_eq!(a.get_local_state(), Some(2.into()));

    let mut b = Awareness::new(2, 1000);
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
        .import(&doc_a.export(loro::ExportMode::Snapshot).unwrap())
        .unwrap();
    doc_a
        .import(&doc_b.export(loro::ExportMode::Snapshot).unwrap())
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
    doc2.import(&updates.unwrap()).unwrap();

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
    doc.import(&updates2.unwrap()).unwrap();

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
fn test_shallow_sync() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    apply_random_ops(&doc, 123, 11);
    let bytes = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(
        doc.peer_id(),
        10,
    )));

    let new_doc = LoroDoc::new();
    new_doc.set_peer_id(2).unwrap();
    new_doc.import(&bytes.unwrap()).unwrap();
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());
    let trim_end = new_doc
        .shallow_since_vv()
        .get(&doc.peer_id())
        .copied()
        .unwrap();
    assert_eq!(trim_end, 10);

    apply_random_ops(&new_doc, 1234, 5);
    let updates = new_doc.export(loro::ExportMode::updates_owned(doc.oplog_vv()));
    doc.import(&updates.unwrap()).unwrap();
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());

    apply_random_ops(&doc, 11, 5);
    let updates = doc.export(loro::ExportMode::updates_owned(new_doc.oplog_vv()));
    new_doc.import(&updates.unwrap()).unwrap();
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());
}

#[test]
fn test_shallow_empty() {
    let doc = LoroDoc::new();
    apply_random_ops(&doc, 123, 11);
    let bytes = doc.export(loro::ExportMode::shallow_snapshot(&Frontiers::default()));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap()).unwrap();
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());
    apply_random_ops(&new_doc, 0, 10);
    doc.import(&new_doc.export_from(&Default::default()))
        .unwrap();
    assert_eq!(doc.get_deep_value(), new_doc.get_deep_value());

    let bytes = new_doc.export(loro::ExportMode::Snapshot);
    let doc_c = LoroDoc::new();
    doc_c.import(&bytes.unwrap()).unwrap();
    assert_eq!(doc_c.get_deep_value(), new_doc.get_deep_value());
}

#[test]
fn test_shallow_import_outdated_updates() {
    let doc = LoroDoc::new();
    apply_random_ops(&doc, 123, 11);
    let bytes = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(
        doc.peer_id(),
        5,
    )));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap()).unwrap();

    let other_doc = LoroDoc::new();
    apply_random_ops(&other_doc, 123, 11);
    let err = new_doc
        .import(&other_doc.export_from(&Default::default()))
        .unwrap_err();
    assert_eq!(err, LoroError::ImportUpdatesThatDependsOnOutdatedVersion);
}

#[test]
fn test_shallow_import_pending_updates_that_is_outdated() {
    let doc = LoroDoc::new();
    apply_random_ops(&doc, 123, 11);
    let bytes = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(
        doc.peer_id(),
        5,
    )));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap()).unwrap();

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
fn test_calling_exporting_snapshot_on_shallow_doc() {
    let doc = LoroDoc::new();
    apply_random_ops(&doc, 123, 11);
    let bytes = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(
        doc.peer_id(),
        5,
    )));
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap()).unwrap();
    let snapshot = new_doc.export(loro::ExportMode::Snapshot);
    let doc_c = LoroDoc::new();
    doc_c.import(&snapshot.unwrap()).unwrap();
    assert_eq!(doc_c.get_deep_value(), new_doc.get_deep_value());
    assert_eq!(new_doc.shallow_since_vv(), doc_c.shallow_since_vv());
}

#[test]
fn sync_two_shallow_docs() {
    let doc = LoroDoc::new();
    apply_random_ops(&doc, 123, 11);
    let bytes = doc
        .export(loro::ExportMode::shallow_snapshot_since(ID::new(
            doc.peer_id(),
            10,
        )))
        .unwrap();

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
    assert_eq!(doc_a.shallow_since_vv(), doc_b.shallow_since_vv());
}

#[test]
fn test_map_checkout_on_shallow_doc() {
    let doc = LoroDoc::new();
    doc.get_map("map").insert("0", 0).unwrap();
    doc.get_map("map").insert("1", 1).unwrap();
    doc.get_map("map").insert("2", 2).unwrap();
    doc.get_map("map").insert("3", 3).unwrap();
    doc.get_map("map").insert("2", 4).unwrap();

    let new_doc_bytes = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(
        doc.peer_id(),
        1,
    )));

    let new_doc = LoroDoc::new();
    new_doc.import(&new_doc_bytes.unwrap()).unwrap();
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
    assert_eq!(err, LoroError::SwitchToVersionBeforeShallowRoot);
}

#[test]
fn test_loro_export_local_updates() {
    use std::sync::{Arc, Mutex};

    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    let updates = Arc::new(Mutex::new(Vec::new()));

    let updates_clone = updates.clone();
    let subscription = doc.subscribe_local_update(Box::new(move |bytes: &Vec<u8>| {
        updates_clone.lock().unwrap().push(bytes.to_vec());
        true
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

#[test]
fn test_movable_list_checkout_on_shallow_doc() -> LoroResult<()> {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    list.insert(0, 0)?;
    list.set(0, 1)?;
    list.set(0, 3)?;
    list.insert(1, 2)?;
    list.mov(1, 0)?;
    list.delete(0, 1)?;
    list.set(0, 0)?;
    let new_doc_bytes = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(
        doc.peer_id(),
        2,
    )));

    let new_doc = LoroDoc::new();
    new_doc.import(&new_doc_bytes.unwrap()).unwrap();
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
    assert_eq!(err, LoroError::SwitchToVersionBeforeShallowRoot);
    Ok(())
}

#[test]
fn test_tree_checkout_on_shallow_doc() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(0)?;
    let tree = doc.get_tree("tree");
    tree.enable_fractional_index(0);
    let root = tree.create(None)?;
    let child1 = tree.create(None)?;
    tree.mov(child1, root)?;
    let child2 = tree.create(None).unwrap();
    tree.mov(child2, root)?;

    let new_doc_bytes = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(
        doc.peer_id(),
        1,
    )));

    let new_doc = LoroDoc::new();
    new_doc.import(&new_doc_bytes.unwrap()).unwrap();
    assert_eq!(
        new_doc.get_deep_value(),
        loro_value!({
            "tree": [
                {
                    "parent": null,
                    "meta":{},
                    "id": "0@0",
                    "index": 0,
                    "children": [{
                        "parent": "0@0",
                        "meta":{},
                        "id": "1@0",
                        "index": 0,
                        "children": [],
                        "fractional_index": "80",
                    },{
                        "parent": "0@0",
                        "meta":{},
                        "id": "3@0",
                        "index": 1,
                        "children": [],
                        "fractional_index": "8180",
                    },],
                    "fractional_index": "80",
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
                    "children":[{
                        "parent": "0@0",
                        "meta":{},
                        "id": "1@0",
                        "index": 0,
                        "children": [],
                        "fractional_index": "80",
                    }],
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
                    "children": [],
                    "fractional_index": "80",
                },
                {
                    "parent": null,
                    "meta":{},
                    "id": "1@0",
                    "index": 1,
                    "children": [],
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
                    "children": [
                        {
                            "parent": "0@0",
                            "meta":{},
                            "id": "1@0",
                            "index": 0,
                            "children": [],
                            "fractional_index": "80",
                        },
                        {
                            "parent": "0@0",
                            "meta":{},
                            "id": "3@0",
                            "index": 1,
                            "children": [],
                            "fractional_index": "8180",
                        },
                    ],
                    "fractional_index": "80",
                },

            ]
        })
    );

    let err = new_doc
        .checkout(&ID::new(doc.peer_id(), 0).into())
        .unwrap_err();
    assert_eq!(err, LoroError::SwitchToVersionBeforeShallowRoot);
    Ok(())
}

#[test]
fn test_tree_with_other_ops_checkout_on_shallow_doc() -> LoroResult<()> {
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
    let shallow_frontiers = doc.oplog_frontiers();
    map.insert("2", 2)?;
    tree.mov(child2, child1)?;
    tree.delete(child1)?;

    let new_doc_bytes = doc.export(loro::ExportMode::shallow_snapshot(&shallow_frontiers));

    let new_doc = LoroDoc::new();
    new_doc.import(&new_doc_bytes.unwrap()).unwrap();

    new_doc.checkout(&shallow_frontiers)?;
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
                    "children": [{
                        "parent": "0@0",
                        "meta":{},
                        "id": "1@0",
                        "index": 0,
                        "children": [],
                        "fractional_index": "80",
                    },
                    {
                        "parent": "0@0",
                        "meta":{},
                        "id": "3@0",
                        "index": 1,
                        "children": [],
                        "fractional_index": "8180",
                    },],
                    "fractional_index": "80",
                },

            ]
            }
        )
    );

    let err = new_doc
        .checkout(&ID::new(doc.peer_id(), 0).into())
        .unwrap_err();
    assert_eq!(err, LoroError::SwitchToVersionBeforeShallowRoot);
    Ok(())
}

#[test]
fn test_shallow_can_remove_unreachable_states() -> LoroResult<()> {
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
        let snapshot = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 3)));
        let new_doc = LoroDoc::new();
        new_doc.import(&snapshot.unwrap())?;
        let a = new_doc.analyze();
        assert_eq!(a.len(), 4);
        assert_eq!(a.dropped_len(), 3);
        new_doc.checkout(&Frontiers::from(ID::new(1, 3))).unwrap();
        let a = new_doc.analyze();
        assert_eq!(a.len(), 4);
        assert_eq!(a.dropped_len(), 0);
    }

    {
        let snapshot = doc.export(loro::ExportMode::shallow_snapshot_since(ID::new(1, 4)));
        let new_doc = LoroDoc::new();
        new_doc.import(&snapshot.unwrap())?;
        assert_eq!(new_doc.analyze().dropped_len(), 0);
    }

    Ok(())
}

#[test]
fn small_update_size() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    text.insert(0, "h").unwrap();
    let bytes = doc.export(loro::ExportMode::all_updates()).unwrap();
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
    let delta = text.get_richtext_value();
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
    new_doc.import(&snapshot.unwrap()).unwrap();
    assert_eq!(new_doc.state_frontiers(), f);
    assert_eq!(
        new_doc.get_deep_value().to_json_value(),
        json!({
            "list": [""]
        })
    );
    new_doc
        .import(&doc.export(loro::ExportMode::all_updates()).unwrap())
        .unwrap();
    assert_eq!(
        new_doc.get_deep_value().to_json_value(),
        json!({
            "list": [1]
        })
    );
}

#[test]
fn change_peer_id() {
    use std::sync::atomic::Ordering;
    let doc = LoroDoc::new();
    let received_peer_id = Arc::new(AtomicU64::new(0));
    let received_peer_id_clone = received_peer_id.clone();
    let sub = doc.subscribe_peer_id_change(Box::new(move |id| {
        received_peer_id_clone.store(id.peer, Ordering::SeqCst);
        true
    }));

    doc.set_peer_id(1).unwrap();
    assert_eq!(received_peer_id.load(Ordering::SeqCst), 1);
    doc.set_peer_id(2).unwrap();
    assert_eq!(received_peer_id.load(Ordering::SeqCst), 2);
    doc.set_peer_id(3).unwrap();
    assert_eq!(received_peer_id.load(Ordering::SeqCst), 3);
    sub.unsubscribe();
    doc.set_peer_id(4).unwrap();
    assert_eq!(received_peer_id.load(Ordering::SeqCst), 3);
}

#[test]
fn test_encode_snapshot_when_checkout() {
    let doc = LoroDoc::new();
    doc.get_text("text").insert(0, "Hello").unwrap();
    doc.commit();
    let f = doc.state_frontiers();
    doc.get_text("text").insert(5, " World").unwrap();
    doc.commit();
    doc.checkout(&f).unwrap();
    let snapshot = doc.export(loro::ExportMode::snapshot());
    let new_doc = LoroDoc::new();
    new_doc.import(&snapshot.unwrap()).unwrap();
    assert_eq!(
        new_doc.get_deep_value().to_json_value(),
        json!({"text": "Hello World"})
    );
}

#[test]
fn test_travel_change_ancestors() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.get_text("text").insert(0, "Hello").unwrap();
    doc.commit();
    let doc2 = doc.fork();
    doc2.set_peer_id(2).unwrap();
    doc2.get_text("text").insert(5, " World").unwrap();
    doc.get_text("text").insert(5, " Alice").unwrap();
    doc.import(&doc2.export(loro::ExportMode::all_updates()).unwrap())
        .unwrap();
    doc2.import(&doc.export(loro::ExportMode::all_updates()).unwrap())
        .unwrap();

    doc.get_text("text").insert(0, "Y").unwrap();
    doc2.get_text("text").insert(0, "N").unwrap();
    doc.commit();
    doc2.commit();
    doc.import(&doc2.export(loro::ExportMode::all_updates()).unwrap())
        .unwrap();
    doc.get_text("text").insert(0, "X").unwrap();
    doc.commit();
    let f = doc.state_frontiers();
    assert_eq!(f.len(), 1);
    let mut changes = vec![];
    doc.travel_change_ancestors(&[f.iter().next().unwrap()], &mut |meta| {
        changes.push(meta.clone());
        ControlFlow::Continue(())
    })
    .unwrap();

    let dbg_str = format!("{:#?}", changes);
    pretty_assertions::assert_eq!(
        dbg_str,
        r#"[
    ChangeMeta {
        lamport: 12,
        id: 12@1,
        timestamp: 0,
        message: None,
        deps: Frontiers(
            [
                11@1,
                6@2,
            ],
        ),
        len: 1,
    },
    ChangeMeta {
        lamport: 11,
        id: 6@2,
        timestamp: 0,
        message: None,
        deps: Frontiers(
            [
                10@1,
                5@2,
            ],
        ),
        len: 1,
    },
    ChangeMeta {
        lamport: 11,
        id: 11@1,
        timestamp: 0,
        message: None,
        deps: Frontiers(
            [
                10@1,
                5@2,
            ],
        ),
        len: 1,
    },
    ChangeMeta {
        lamport: 5,
        id: 0@2,
        timestamp: 0,
        message: None,
        deps: Frontiers(
            [
                4@1,
            ],
        ),
        len: 6,
    },
    ChangeMeta {
        lamport: 0,
        id: 0@1,
        timestamp: 0,
        message: None,
        deps: Frontiers(
            [],
        ),
        len: 11,
    },
]"#
    );

    let mut changes = vec![];
    doc.travel_change_ancestors(&[ID::new(2, 4)], &mut |meta| {
        changes.push(meta.clone());
        ControlFlow::Continue(())
    })
    .unwrap();
    let dbg_str = format!("{:#?}", changes);
    assert_eq!(
        dbg_str,
        r#"[
    ChangeMeta {
        lamport: 5,
        id: 0@2,
        timestamp: 0,
        message: None,
        deps: Frontiers(
            [
                4@1,
            ],
        ),
        len: 6,
    },
    ChangeMeta {
        lamport: 0,
        id: 0@1,
        timestamp: 0,
        message: None,
        deps: Frontiers(
            [],
        ),
        len: 11,
    },
]"#
    );
}

#[test]
fn no_dead_loop_when_subscribe_local_updates_to_each_other() {
    let doc1 = Arc::new(LoroDoc::new());
    let doc2 = Arc::new(LoroDoc::new());

    let doc1_clone = doc1.clone();
    let doc2_clone = doc2.clone();
    let _sub1 = doc1.subscribe_local_update(Box::new(move |updates| {
        doc2_clone.import(updates).unwrap();
        true
    }));
    let _sub2 = doc2.subscribe_local_update(Box::new(move |updates| {
        doc1_clone.import(updates).unwrap();
        true
    }));

    doc1.get_text("text").insert(0, "Hello").unwrap();
    doc1.commit();
    doc2.get_text("text").insert(0, "World").unwrap();
    doc2.commit();

    assert_eq!(doc1.get_deep_value(), doc2.get_deep_value());
}

/// https://github.com/loro-dev/loro/issues/490
#[test]
fn issue_490() -> anyhow::Result<()> {
    let fx_loro = loro::LoroDoc::new();
    fx_loro
        .get_map("paciente")
        .insert("nome", "DUMMY NAME V0")?;
    fx_loro.commit();

    let loro_c1 = fx_loro.fork();
    loro_c1
        .get_map("paciente")
        .insert("nome", "DUMMY NAME V1")?;
    loro_c1.commit();

    // If I use `fork()` it panics
    let final_loro = fx_loro.fork();

    // If I create a new loro doc and import the snapshot it works
    //let final_loro = loro::LoroDoc::new();
    //final_loro.import(&fx_loro.export(loro::ExportMode::snapshot())?)?;

    final_loro.import(&loro_c1.export(loro::ExportMode::snapshot())?)?;
    Ok(())
}

#[test]
fn test_loro_doc() {
    let doc = LoroDoc::new();
    doc.get_text("text").insert(0, "Hello").unwrap();
    doc.state_vv();
}

#[test]
fn test_fork_at_should_restore_attached_state() {
    let doc = LoroDoc::new();
    doc.set_peer_id(0).unwrap();
    doc.get_text("text").insert(0, "Hello").unwrap();
    doc.fork_at(&[ID::new(0, 0)].into());
    assert!(!doc.is_detached());
    doc.detach();
    doc.fork_at(&[ID::new(0, 0)].into());
    assert!(doc.is_detached());
}

#[test]
fn test_fork_when_detached() {
    let doc = LoroDoc::new();
    doc.set_peer_id(0).unwrap();
    doc.get_text("text").insert(0, "Hello, world!").unwrap();
    doc.checkout(&[ID::new(0, 5)].into()).unwrap();
    let new_doc = doc.fork();
    new_doc.set_peer_id(1).unwrap();
    new_doc.get_text("text").insert(6, " Alice!").unwrap();
    // ┌───────────────┐     ┌───────────────┐
    // │    Hello,     │◀─┬──│     world!    │
    // └───────────────┘  │  └───────────────┘
    //                    │
    //                    │  ┌───────────────┐
    //                    └──│     Alice!    │
    //                       └───────────────┘
    doc.import(&new_doc.export(loro::ExportMode::all_updates()).unwrap())
        .unwrap();
    doc.checkout_to_latest();
    assert_eq!(doc.get_text("text").to_string(), "Hello, world! Alice!");
}

#[test]
fn test_for_each_movable_list() {
    let doc = LoroDoc::new();
    let list = doc.get_movable_list("list");
    list.insert(0, 1).unwrap();
    list.insert(1, "hello").unwrap();
    list.insert(2, true).unwrap();
    let mut vec = vec![];
    list.for_each(|v| {
        vec.push(v.into_value().unwrap());
    });
    assert_eq!(vec, vec![1.into(), "hello".into(), true.into()]);
}

#[test]
fn test_for_each_map() {
    let doc = LoroDoc::new();
    let map = doc.get_map("map");
    map.insert("0", 0).unwrap();
    map.insert("1", 1).unwrap();
    map.insert("2", 2).unwrap();
    let mut vec = vec![];
    map.for_each(|_, v| {
        vec.push(v.into_value().unwrap());
    });
    assert_eq!(vec, vec![0.into(), 1.into(), 2.into()]);
}

#[test]
fn test_for_each_list() {
    let doc = LoroDoc::new();
    let list = doc.get_list("list");
    list.insert(0, 0).unwrap();
    list.insert(1, 1).unwrap();
    list.insert(2, 2).unwrap();
    let mut vec = vec![];
    list.for_each(|v| {
        vec.push(v.into_value().unwrap());
    });
    assert_eq!(vec, vec![0.into(), 1.into(), 2.into()]);
}

#[test]
#[should_panic]
fn should_avoid_initialize_new_container_accidentally() {
    let doc = LoroDoc::new();
    let id = ContainerID::new_normal(ID::new(0, 0), ContainerType::Text);
    let _text = doc.get_text(id);
}

#[test]
fn test_decode_import_blob_meta_mode() {
    let doc0 = LoroDoc::new();
    doc0.set_peer_id(0).unwrap();
    doc0.get_text("text").insert(0, "Hello").unwrap();
    doc0.commit_with(CommitOptions::default().timestamp(100));
    let blob = doc0.export(loro::ExportMode::snapshot()).unwrap();
    let meta = LoroDoc::decode_import_blob_meta(&blob, false).unwrap();
    assert_eq!(meta.mode, EncodedBlobMode::Snapshot);
    assert_eq!(meta.end_timestamp, 100);
    assert_eq!(meta.change_num, 1);
    assert_eq!(meta.start_frontiers.len(), 0);
    assert_eq!(meta.partial_start_vv.len(), 0);

    // Check Updates mode
    let doc1 = LoroDoc::new();
    doc1.set_peer_id(1).unwrap();
    doc1.get_text("text").insert(0, "World").unwrap();
    let blob = doc1
        .export(loro::ExportMode::Updates {
            from: Default::default(),
        })
        .unwrap();
    let meta = LoroDoc::decode_import_blob_meta(&blob, false).unwrap();
    assert_eq!(meta.mode, EncodedBlobMode::Updates);

    // Check ShallowSnapshot mode
    let blob = doc0
        .export(loro::ExportMode::ShallowSnapshot(std::borrow::Cow::Owned(
            doc0.state_frontiers(),
        )))
        .unwrap();
    let meta = LoroDoc::decode_import_blob_meta(&blob, false).unwrap();
    assert_eq!(meta.mode, EncodedBlobMode::ShallowSnapshot);

    // Check StateOnly mode
    let blob = doc0.export(loro::ExportMode::StateOnly(None)).unwrap();
    let meta = LoroDoc::decode_import_blob_meta(&blob, false).unwrap();
    assert_eq!(meta.mode, EncodedBlobMode::ShallowSnapshot);

    // Check SnapshotAt mode
    let blob = doc0
        .export(loro::ExportMode::SnapshotAt {
            version: std::borrow::Cow::Owned(doc0.state_frontiers()),
        })
        .unwrap();
    let meta = LoroDoc::decode_import_blob_meta(&blob, false).unwrap();
    assert_eq!(meta.mode, EncodedBlobMode::Snapshot);
}

#[test]
fn test_decode_import_blob_meta_shallow_since() {
    let doc = LoroDoc::new();
    doc.set_peer_id(0).unwrap();
    doc.get_text("t").insert(0, "12345").unwrap();
    doc.commit_with(CommitOptions::default().timestamp(10));
    let bytes = doc
        .export(ExportMode::shallow_snapshot(&ID::new(0, 3).into()))
        .unwrap();
    let meta = LoroDoc::decode_import_blob_meta(&bytes, false).unwrap();
    assert_eq!(meta.start_frontiers, Frontiers::from(vec![ID::new(0, 3)]));
    assert_eq!(meta.partial_start_vv, vv!(0 => 3));
    assert_eq!(meta.partial_end_vv, vv!(0 => 5));
    assert_eq!(meta.start_timestamp, 10);
}

#[test]
fn test_decode_import_blob_meta_updates_range() {
    let doc = LoroDoc::new();
    doc.set_peer_id(0).unwrap();
    doc.get_text("t").insert(0, "12345").unwrap();
    doc.set_peer_id(1).unwrap();
    doc.get_text("t").insert(0, "67890").unwrap();
    let bytes = doc
        .export(ExportMode::updates(&vv!(0 => 1, 1 => 1)))
        .unwrap();
    let meta = LoroDoc::decode_import_blob_meta(&bytes, false).unwrap();
    assert_eq!(meta.mode, EncodedBlobMode::Updates);
    assert_eq!(meta.partial_start_vv, vv!(0 => 1, 1 => 1));
    assert_eq!(meta.partial_end_vv, vv!(0 => 5, 1 => 5));
}

#[test]
fn should_import_snapshot_before_shallow_snapshot() {
    let doc = LoroDoc::new();
    doc.set_peer_id(0).unwrap();
    doc.get_text("text").insert(0, "Hello").unwrap();
    doc.commit();
    let snapshot = doc.export(ExportMode::Snapshot).unwrap();
    let shallow = doc
        .export(ExportMode::shallow_snapshot(&ID::new(0, 4).into()))
        .unwrap();

    let doc2 = LoroDoc::new();
    let blobs = vec![shallow, snapshot];
    doc2.import_batch(&blobs).unwrap();
    assert!(!doc2.is_shallow());
}

#[test]
fn get_last_editor_on_map() {
    let doc = LoroDoc::new();
    doc.set_peer_id(0).unwrap();
    let map = doc.get_map("map");
    map.insert("key1", "value1").unwrap();
    assert_eq!(map.get_last_editor("key1"), Some(0));
    doc.set_peer_id(1).unwrap();
    map.insert("key1", "value2").unwrap();
    map.insert("key2", "value3").unwrap();

    assert_eq!(map.get_last_editor("key1"), Some(1));
    assert_eq!(map.get_last_editor("key2"), Some(1));
    assert_eq!(map.get_last_editor("nonexistent"), None);
}

#[test]
fn get_editor() {
    let doc = LoroDoc::new();
    doc.set_peer_id(0).unwrap();
    let text = doc.get_text("text");
    text.insert(0, "01234").unwrap();
    assert_eq!(text.get_editor_at_unicode_pos(3), Some(0));
    let list = doc.get_list("list");
    list.insert(0, 0).unwrap();
    assert_eq!(list.get_id_at(0).unwrap().peer, 0);
    let mov_list = doc.get_movable_list("mov_list");
    mov_list.insert(0, 0).unwrap();
    mov_list.insert(1, 0).unwrap();
    mov_list.set(0, 1).unwrap();
    doc.set_peer_id(1).unwrap();
    mov_list.mov(0, 1).unwrap();
    assert_eq!(mov_list.get_creator_at(0), Some(0));
    assert_eq!(mov_list.get_last_mover_at(0), Some(0));
    assert_eq!(mov_list.get_last_mover_at(1), Some(1));
    assert_eq!(mov_list.get_last_editor_at(1), Some(0));

    let tree = doc.get_tree("tree");
    let node_0 = tree.create(None).unwrap();
    let node_1 = tree.create(None).unwrap();
    let mov_id = tree.get_last_move_id(&node_0).unwrap();
    assert_eq!(mov_id.peer, 1);
    doc.set_peer_id(2).unwrap();
    tree.mov(node_0, node_1).unwrap();
    let mov_id = tree.get_last_move_id(&node_0).unwrap();
    assert_eq!(mov_id.peer, 2);
}

#[test]
fn get_changed_containers_in() {
    let doc = LoroDoc::new();
    doc.set_peer_id(0).unwrap();
    let text = doc.get_text("text");
    text.insert(0, "H").unwrap();
    let map = doc.get_map("map");
    map.insert("key", "value").unwrap();
    let changed_set = doc.get_changed_containers_in(ID::new(0, 0), 2);
    assert_eq!(
        changed_set,
        vec![
            ContainerID::new_root("text", ContainerType::Text),
            ContainerID::new_root("map", ContainerType::Map),
        ]
        .into_iter()
        .collect()
    );

    map.insert("key1", "value1").unwrap();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "text": "H",
            "map": {
                "key": "value",
                "key1": "value1"
            }
        })
    )
}

#[test]
fn is_deleted() {
    let doc = LoroDoc::new();
    let list = doc.get_list("list");
    assert!(!list.is_deleted());
    let tree = doc.get_tree("root");
    let node = tree.create(None).unwrap();
    let map = tree.get_meta(node).unwrap();
    let container_before = map.insert_container("container", LoroMap::new()).unwrap();
    container_before.insert("A", "B").unwrap();
    tree.delete(node).unwrap();
    let container_after = doc.get_map(container_before.id());
    assert!(container_after.is_deleted());
}

#[test]
fn change_count() {
    let doc = LoroDoc::new();
    let n = 1024 * 5;
    for i in 0..n {
        doc.get_text("text").insert(0, "H").unwrap();
        doc.set_next_commit_message(&format!("{}", i));
        doc.commit();
    }

    doc.compact_change_store();
    assert_eq!(doc.len_changes(), n);
    let bytes = doc.export(loro::ExportMode::Snapshot);
    let new_doc = LoroDoc::new();
    new_doc.import(&bytes.unwrap()).unwrap();
    assert_eq!(new_doc.len_changes(), n);
}

#[test]
fn loro_import_batch_status() {
    let doc_1 = LoroDoc::new();
    doc_1.set_peer_id(1).unwrap();
    doc_1.get_text("text").insert(0, "Hello world!").unwrap();

    let doc_2 = LoroDoc::new();
    doc_2.set_peer_id(2).unwrap();
    doc_2.get_text("text").insert(0, "Hello world!").unwrap();

    let blob11 = doc_1
        .export(ExportMode::updates_in_range(vec![IdSpan::new(1, 0, 5)]))
        .unwrap();
    let blob12 = doc_1
        .export(ExportMode::updates_in_range(vec![IdSpan::new(1, 5, 7)]))
        .unwrap();
    let blob13 = doc_1
        .export(ExportMode::updates_in_range(vec![IdSpan::new(1, 6, 12)]))
        .unwrap();

    let blob21 = doc_2
        .export(ExportMode::updates_in_range(vec![IdSpan::new(2, 0, 5)]))
        .unwrap();
    let blob22 = doc_2
        .export(ExportMode::updates_in_range(vec![IdSpan::new(2, 5, 6)]))
        .unwrap();
    let blob23 = doc_2
        .export(ExportMode::updates_in_range(vec![IdSpan::new(2, 6, 12)]))
        .unwrap();

    let new_doc = LoroDoc::new();
    let status = new_doc
        .import_batch(&[blob11, blob13, blob21, blob23])
        .unwrap();

    assert_eq!(status.success, version_range!(1 => (0, 5), 2 => (0, 5)));
    assert_eq!(
        status.pending,
        Some(version_range!(1 => (6, 12), 2 => (6, 12)))
    );

    let status = new_doc.import_batch(&[blob12, blob22]).unwrap();
    assert_eq!(status.success, version_range!(1 => (5, 12), 2 => (5, 12)));
    assert!(status.pending.is_none());
    assert_eq!(
        new_doc.get_text("text").to_string(),
        "Hello world!Hello world!"
    );
}

#[test]
fn test_get_or_create_container_with_null() {
    let doc = LoroDoc::new();
    let root = doc.get_map("root");

    // No key -- should work
    root.get_or_create_container("key", LoroMap::new()).unwrap();
    doc.commit();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({ "root": { "key": {} } })
    );

    // Set to null -- should work
    root.insert("key", LoroValue::Null).unwrap();
    doc.commit();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({ "root": { "key": null } })
    );

    // Key is null, create a container -- should work
    let result = root.get_or_create_container("key", LoroMap::new());
    result.unwrap();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({ "root": { "key": {} } })
    );
}

#[test]
fn test_detach_and_attach() {
    let doc = LoroDoc::new();
    assert!(!doc.is_detached());
    doc.detach();
    assert!(doc.is_detached());
    doc.attach();
    assert!(!doc.is_detached());
}

#[test]
fn test_event_order() {
    let doc = LoroDoc::new();
    let _sub = doc.subscribe_root(Arc::new(|e| {
        let e0 = &e.events[0].diff;
        assert!(e0.is_map());
        let e1 = &e.events[1].diff;
        assert!(e1.is_list());
        let e2 = &e.events[2].diff;
        assert!(e2.is_tree());
    }));
    doc.get_map("map").insert("key", "value").unwrap();
    doc.get_list("list").insert(0, "item").unwrap();
    doc.get_tree("tree").create(None).unwrap();
    doc.commit();
}

#[test]
fn test_rust_get_value_by_path() {
    let doc = LoroDoc::new();
    let tree = doc.get_tree("tree");
    let root = tree.create(None).unwrap();
    let child1 = tree.create(root).unwrap();
    let child2 = tree.create(root).unwrap();
    let grandchild = tree.create(child1).unwrap();

    // Set up metadata for nodes
    tree.get_meta(root).unwrap().insert("name", "root").unwrap();
    tree.get_meta(child1)
        .unwrap()
        .insert("name", "child1")
        .unwrap();
    tree.get_meta(child2)
        .unwrap()
        .insert("name", "child2")
        .unwrap();
    tree.get_meta(grandchild)
        .unwrap()
        .insert("name", "grandchild")
        .unwrap();

    // Test getting values by path
    let root_meta = doc.get_by_str_path(&format!("tree/{}", root)).unwrap();
    let root_name = doc.get_by_str_path(&format!("tree/{}/name", root)).unwrap();
    let child1_meta = doc.get_by_str_path(&format!("tree/{}", child1)).unwrap();
    let child1_name = doc
        .get_by_str_path(&format!("tree/{}/name", child1))
        .unwrap();
    let grandchild_name = doc
        .get_by_str_path(&format!("tree/{}/name", grandchild))
        .unwrap();

    // Verify the values
    assert!(root_meta.into_container().unwrap().is_map());
    assert_eq!(
        root_name.into_value().unwrap().into_string().unwrap(),
        LoroStringValue::from("root")
    );
    assert!(child1_meta.into_container().unwrap().is_map());
    assert_eq!(
        child1_name.into_value().unwrap().into_string().unwrap(),
        LoroStringValue::from("child1")
    );
    assert_eq!(
        grandchild_name.into_value().unwrap().into_string().unwrap(),
        LoroStringValue::from("grandchild")
    );

    // Test non-existent paths
    assert!(doc.get_by_str_path("tree/nonexistent").is_none());
    assert!(doc
        .get_by_str_path(&format!("tree/{}/nonexistent", root))
        .is_none());

    // Verify values accessed by index
    assert_eq!(
        doc.get_by_str_path("tree/0/name")
            .unwrap()
            .into_value()
            .unwrap()
            .into_string()
            .unwrap(),
        LoroStringValue::from("root")
    );
    assert_eq!(
        doc.get_by_str_path("tree/0/0/name")
            .unwrap()
            .into_value()
            .unwrap()
            .into_string()
            .unwrap(),
        LoroStringValue::from("child1")
    );
    assert_eq!(
        doc.get_by_str_path("tree/0/1/name")
            .unwrap()
            .into_value()
            .unwrap()
            .into_string()
            .unwrap(),
        LoroStringValue::from("child2")
    );
    assert_eq!(
        doc.get_by_str_path("tree/0/0/0/name")
            .unwrap()
            .into_value()
            .unwrap()
            .into_string()
            .unwrap(),
        LoroStringValue::from("grandchild")
    );

    // Test invalid index paths
    assert!(doc.get_by_str_path("tree/1").is_none()); // Invalid root index
    assert!(doc.get_by_str_path("tree/0/2").is_none()); // Invalid child index
    assert!(doc.get_by_str_path("tree/0/0/1").is_none()); // Invalid grandchild index
}

#[test]
fn travel_before_commit() -> Result<(), Box<dyn std::error::Error>> {
    let doc = LoroDoc::new();
    let map = doc.get_map("metadata");
    map.insert("key", "value")?;
    let last_frontiers = doc.state_frontiers();
    doc.travel_change_ancestors(&last_frontiers.to_vec(), &mut |_meta| {
        std::ops::ControlFlow::Continue(())
    })?;
    Ok(())
}

#[test]
fn test_export_json_in_id_span() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;

    // Test list operations
    let list = doc.get_list("list");
    list.insert(0, 1)?;
    doc.set_next_commit_message("list");
    doc.commit();

    // Test map operations
    let map = doc.get_map("map");
    map.insert("key1", "value1")?;
    doc.set_next_commit_message("map");
    doc.commit();

    // Test text operations
    let text = doc.get_text("text");
    text.insert(0, "H")?;
    doc.set_next_commit_message("text");
    doc.commit();

    // Export changes for list (first change)
    let changes = doc.export_json_in_id_span(IdSpan::new(1, 0, 1));
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].id.peer, 1);
    assert_eq!(changes[0].id.counter, 0);
    assert!(!changes[0].ops.is_empty());

    // Export changes for map (second change)
    let changes = doc.export_json_in_id_span(IdSpan::new(1, 1, 2));
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].id.peer, 1);
    assert_eq!(changes[0].id.counter, 1);
    assert!(!changes[0].ops.is_empty());

    // Export changes for text (third change)
    let changes = doc.export_json_in_id_span(IdSpan::new(1, 2, 3));
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].id.peer, 1);
    assert_eq!(changes[0].id.counter, 2);
    assert!(!changes[0].ops.is_empty());

    // Export multiple changes
    let changes = doc.export_json_in_id_span(IdSpan::new(1, 0, 3));
    assert_eq!(changes.len(), 3);
    assert_eq!(changes[0].id.counter, 0);
    assert_eq!(changes[1].id.counter, 1);
    assert_eq!(changes[2].id.counter, 2);

    // Test with multiple peers
    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2)?;
    doc2.get_list("list").insert(0, 3)?;
    doc2.commit();
    doc.import(&doc2.export_snapshot())?;

    let changes = doc.export_json_in_id_span(IdSpan::new(2, 0, 1));
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].id.peer, 2);
    assert_eq!(changes[0].id.counter, 0);

    // Test empty span
    let changes = doc.export_json_in_id_span(IdSpan::new(1, 0, 0));
    assert_eq!(changes.len(), 0);

    // Test concurrent operations
    let doc1 = LoroDoc::new();
    doc1.set_peer_id(1)?;
    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2)?;

    // Make concurrent changes
    doc1.get_text("text").insert(0, "Hello")?;
    doc2.get_text("text").insert(0, "World")?;
    doc1.commit();
    doc2.commit();

    // Sync the documents
    doc1.import(&doc2.export_snapshot())?;
    doc2.import(&doc1.export_snapshot())?;

    // Export changes from both peers
    let changes1 = doc1.export_json_in_id_span(IdSpan::new(1, 0, 1));
    let changes2 = doc1.export_json_in_id_span(IdSpan::new(2, 0, 1));
    assert_eq!(changes1.len(), 1);
    assert_eq!(changes2.len(), 1);
    assert_eq!(changes1[0].id.peer, 1);
    assert_eq!(changes2[0].id.peer, 2);

    // Verify that the changes can be imported back
    let doc3 = LoroDoc::new();
    doc3.import(&doc1.export_snapshot())?;
    assert_eq!(
        doc3.get_text("text").to_string(),
        doc1.get_text("text").to_string()
    );

    Ok(())
}

#[test]
fn test_export_json_in_id_span_with_complex_operations() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;

    // Test nested container operations
    let map = doc.get_map("root");
    let list = map.insert_container("list", LoroList::new())?;
    list.insert(0, 1)?;
    let text = list.insert_container(1, LoroText::new())?;
    text.insert(0, "Hello")?;
    doc.commit();

    // Export the changes
    let changes = doc.export_json_in_id_span(IdSpan::new(1, 0, 1));
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].id.peer, 1);
    assert_eq!(changes[0].id.counter, 0);
    assert!(!changes[0].ops.is_empty());

    // Test tree operations
    let tree = doc.get_tree("tree");
    let root = tree.create(None)?;
    let child1 = tree.create(None)?;
    let child2 = tree.create(None)?;
    tree.mov(child1, root)?;
    tree.mov(child2, root)?;
    doc.commit();

    // Export tree changes
    let changes = doc.export_json_in_id_span(IdSpan::new(1, 1, 2));
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].id.peer, 1);
    assert_eq!(changes[0].id.counter, 1);
    assert!(!changes[0].ops.is_empty());

    // Test rich text operations with multiple attributes
    let text = doc.get_text("richtext");
    text.insert(0, "Hello World")?;
    text.mark(0..5, "bold", true)?;
    text.mark(6..11, "italic", true)?;
    doc.commit();

    // Export rich text changes
    let changes = doc.export_json_in_id_span(IdSpan::new(1, 2, 3));
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].id.peer, 1);
    assert_eq!(changes[0].id.counter, 2);
    assert!(!changes[0].ops.is_empty());

    // Test movable list operations
    let movable_list = doc.get_movable_list("movable");
    movable_list.insert(0, 1)?;
    movable_list.insert(1, 2)?;
    movable_list.mov(0, 1)?;
    doc.commit();

    // Export movable list changes
    let changes = doc.export_json_in_id_span(IdSpan::new(1, 3, 4));
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].id.peer, 1);
    assert_eq!(changes[0].id.counter, 3);
    assert!(!changes[0].ops.is_empty());

    // Verify that all changes can be imported back
    let doc2 = LoroDoc::new();
    doc2.import(&doc.export_snapshot())?;
    assert_eq!(doc2.get_deep_value(), doc.get_deep_value());

    Ok(())
}

#[test]
fn test_find_spans_between() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;

    // Make some changes to create version history
    doc.get_text("text").insert(0, "Hello")?;
    doc.set_next_commit_message("a");
    doc.commit();
    let f1 = doc.state_frontiers();

    doc.get_text("text").insert(5, " World")?;
    doc.set_next_commit_message("b");
    doc.commit();
    let f2 = doc.state_frontiers();

    // Test finding spans between frontiers (f1 -> f2)
    let diff = doc.find_id_spans_between(&f1, &f2);
    assert!(diff.retreat.is_empty()); // No changes needed to go from f2 to f1
    assert_eq!(diff.forward.len(), 1); // One change needed to go from f1 to f2
    let span = diff.forward.get(&1).unwrap();
    assert_eq!(span.start, 5); // First change ends at counter 3
    assert_eq!(span.end, 11); // Second change ends at counter 6

    // Test empty frontiers
    let empty_frontiers = Frontiers::default();
    let diff = doc.find_id_spans_between(&empty_frontiers, &f2);
    assert!(diff.retreat.is_empty()); // No changes needed to go from f2 to empty
    assert_eq!(diff.forward.len(), 1); // One change needed to go from empty to f2
    let span = diff.forward.get(&1).unwrap();
    assert_eq!(span.start, 0); // From beginning
    assert_eq!(span.end, 11); // To latest change

    // Test with multiple peers
    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2)?;
    doc2.get_text("text").insert(0, "Hi")?;
    doc2.commit();
    doc.import(&doc2.export_snapshot())?;
    let f3 = doc.state_frontiers();

    // Test finding spans between f2 and f3
    let diff = doc.find_id_spans_between(&f2, &f3);
    assert!(diff.retreat.is_empty()); // No changes needed to go from f3 to f2
    assert_eq!(diff.forward.len(), 1); // One change needed to go from f2 to f3
    let span = diff.forward.get(&2).unwrap();
    assert_eq!(span.start, 0);
    assert_eq!(span.end, 2);

    // Test spans in both directions between f1 and f3
    let diff = doc.find_id_spans_between(&f1, &f3);
    assert!(diff.retreat.is_empty()); // No changes needed to go from f3 to f1
    assert_eq!(diff.forward.len(), 2); // Two changes needed to go from f1 to f3
    for (peer, span) in diff.forward.iter() {
        match peer {
            1 => {
                assert_eq!(span.start, 5);
                assert_eq!(span.end, 11);
            }
            2 => {
                assert_eq!(span.start, 0);
                assert_eq!(span.end, 2);
            }
            _ => panic!("Unexpected peer ID"),
        }
    }

    let diff = doc.find_id_spans_between(&f3, &f1);
    assert!(diff.forward.is_empty()); // No changes needed to go from f3 to f1
    assert_eq!(diff.retreat.len(), 2); // Two changes needed to go from f1 to f3
    for (peer, span) in diff.retreat.iter() {
        match peer {
            1 => {
                assert_eq!(span.start, 5);
                assert_eq!(span.end, 11);
            }
            2 => {
                assert_eq!(span.start, 0);
                assert_eq!(span.end, 2);
            }
            _ => panic!("Unexpected peer ID"),
        }
    }

    Ok(())
}

#[test]
fn revert_to() -> anyhow::Result<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_text("text").insert(0, "Hello")?;
    doc.commit();
    let f1 = doc.state_frontiers();
    doc.get_text("text").insert(5, " World")?;
    doc.commit();
    let f2 = doc.state_frontiers();
    doc.revert_to(&f1)?;
    assert_eq!(doc.get_text("text").to_string(), "Hello");
    for _ in 0..10 {
        doc.get_text("text").insert(0, "12345")?;
        doc.commit();
    }
    doc.get_text("text").delete(0, 50)?;
    doc.commit();
    let f3_counter = doc.state_frontiers().as_single().unwrap().counter;
    doc.revert_to(&f2)?;
    let f4_counter = doc.state_frontiers().as_single().unwrap().counter;
    assert_eq!(f4_counter - f3_counter, 6); // Only need to redo the insertion of " World", other 50 operations should be ignored
    assert_eq!(doc.get_text("text").to_string(), "Hello World");
    Ok(())
}

#[test]
fn test_diff_and_apply_on_another_doc() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_list("list").insert(0, "a")?;
    doc.get_list("list").insert(1, "b")?;
    let f0 = doc.state_frontiers();
    doc.get_map("map").insert("key", "value")?;
    doc.get_list("list").insert(2, "hi")?;
    doc.commit();
    let f1 = doc.state_frontiers();
    let diff = doc.diff(&f0, &f1)?;
    let diff_revert = doc.diff(&f1, &f0)?;

    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2)?;
    doc2.get_list("list").insert(0, 1)?;
    doc2.get_list("list").insert(1, 2)?;
    doc2.commit();
    doc2.apply_diff(diff.clone())?;
    assert_eq!(
        doc2.get_deep_value().to_json_value(),
        json!({"list": [1, 2, "hi"], "map": {"key": "value"}})
    );
    doc2.apply_diff(diff_revert)?;
    assert_eq!(
        doc2.get_deep_value().to_json_value(),
        json!({"list": [1, 2], "map": {}})
    );

    let diff_str = format!("{:#?}", diff);
    assert_eq!(
        diff_str,
        r#"[
    (
        Root("list" List),
        List(
            [
                Retain {
                    retain: 2,
                },
                Insert {
                    insert: [
                        Value(
                            String(
                                LoroStringValue(
                                    "hi",
                                ),
                            ),
                        ),
                    ],
                    is_move: false,
                },
            ],
        ),
    ),
    (
        Root("map" Map),
        Map(
            MapDelta {
                updated: {
                    "key": Some(
                        Value(
                            String(
                                LoroStringValue(
                                    "value",
                                ),
                            ),
                        ),
                    ),
                },
            },
        ),
    ),
]"#
    );
    Ok(())
}

#[test]
fn test_diff_and_apply_on_another_doc_with_child_container() -> LoroResult<()> {
    let doc = LoroDoc::new();
    doc.set_peer_id(1)?;
    doc.get_list("list").insert(0, "a")?;
    doc.get_list("list").insert(1, "b")?;
    let f0 = doc.state_frontiers();
    let text = doc
        .get_map("map")
        .insert_container("key", LoroText::new())?;
    text.insert(0, "value")?;
    doc.get_list("list").insert(2, "hi")?;
    doc.commit();
    let f1 = doc.state_frontiers();
    let diff = doc.diff(&f0, &f1)?;
    let diff_revert = doc.diff(&f1, &f0)?;

    let doc2 = LoroDoc::new();
    doc2.set_peer_id(2)?;
    doc2.get_list("list").insert(0, 1)?;
    doc2.get_list("list").insert(1, 2)?;
    doc2.commit();
    doc2.apply_diff(diff.clone())?;
    assert_eq!(
        doc2.get_deep_value().to_json_value(),
        json!({"list": [1, 2, "hi"], "map": {"key": "value"}})
    );
    doc2.apply_diff(diff_revert)?;
    assert_eq!(
        doc2.get_deep_value().to_json_value(),
        json!({"list": [1, 2], "map": {}})
    );

    let diff_str = format!("{:#?}", diff);
    assert_eq!(
        diff_str,
        r#"[
    (
        Root("list" List),
        List(
            [
                Retain {
                    retain: 2,
                },
                Insert {
                    insert: [
                        Value(
                            String(
                                LoroStringValue(
                                    "hi",
                                ),
                            ),
                        ),
                    ],
                    is_move: false,
                },
            ],
        ),
    ),
    (
        Root("map" Map),
        Map(
            MapDelta {
                updated: {
                    "key": Some(
                        Container(
                            Text(
                                LoroText {
                                    handler: TextHandler(Normal(Text 2@1)),
                                },
                            ),
                        ),
                    ),
                },
            },
        ),
    ),
    (
        Normal(Text 2@1),
        Text(
            [
                Insert {
                    insert: "value",
                    attributes: None,
                },
            ],
        ),
    ),
]"#
    );
    Ok(())
}

#[test]
fn test_diff_apply_with_unknown_container() -> LoroResult<()> {
    let doc = LoroDoc::new();
    let mut batch = DiffBatch::default();
    batch
        .push(
            ContainerID::new_normal(ID::new(1, 1), ContainerType::List),
            Diff::List(vec![ListDiffItem::Delete { delete: 5 }]),
        )
        .unwrap();
    let ans = doc.apply_diff(batch);
    assert!(ans.is_err());
    assert!(matches!(
        ans,
        Err(LoroError::ContainersNotFound { containers: _ }),
    ),);
    Ok(())
}

#[test]
fn test_set_merge_interval() {
    let doc = LoroDoc::new();
    doc.set_record_timestamp(true);
    doc.set_change_merge_interval(1);
    doc.get_text("text").insert(0, "Hello").unwrap();
    doc.commit_with(CommitOptions::default().timestamp(100));
    doc.get_text("text").insert(0, "Hello").unwrap();
    doc.commit_with(CommitOptions::default().timestamp(200));
    assert_eq!(doc.len_changes(), 2);
    {
        let snapshot = doc.export(ExportMode::Snapshot).unwrap();
        let new_doc = LoroDoc::new();
        new_doc.import(&snapshot).unwrap();
        assert_eq!(new_doc.len_changes(), 2);
    }
    {
        let updates = doc.export(ExportMode::all_updates()).unwrap();
        let new_doc = LoroDoc::new();
        new_doc.import(&updates).unwrap();
        assert_eq!(new_doc.len_changes(), 2);
    }
}

#[test]
fn test_child_container_attach_behavior() {
    let map = LoroMap::new();
    let child = map.insert_container("child", LoroMap::new()).unwrap();
    let doc = LoroDoc::new();
    doc.get_map("meta").insert_container("map", map).unwrap();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "meta": { "map": { "child": {} } }
        })
    );
    let attached = child.get_attached().unwrap();
    attached.insert("key", "value").unwrap();
    assert_eq!(
        doc.get_deep_value().to_json_value(),
        json!({
            "meta": { "map": { "child": { "key": "value" } } }
        })
    );
}

#[test]
fn test_map_keys_values_for_each() {
    let doc = LoroDoc::new();
    let map = doc.get_map("map");
    map.insert("a", "b").unwrap();
    map.insert("c", "d").unwrap();
    map.insert("e", "f").unwrap();
    map.delete("c").unwrap();
    let mut keys = HashSet::new();
    let mut values = HashSet::new();
    map.for_each(|k, v| {
        keys.insert(k.to_string());
        values.insert(v.into_value().unwrap().into_string().unwrap().to_string());
    });
    let keys2 = map.keys().map(|k| k.to_string()).collect::<HashSet<_>>();
    let values2 = map
        .values()
        .map(|v| v.into_value().unwrap().into_string().unwrap().to_string())
        .collect::<HashSet<_>>();
    assert_eq!(keys, keys2);
    assert_eq!(values, values2);
}

#[test]
fn test_update_long_text() {
    let text = "a".repeat(1_000_000);
    let doc = LoroDoc::new();
    doc.get_text("text")
        .update(&text, Default::default())
        .unwrap();
    assert_eq!(doc.get_text("text").to_string(), text);
}

#[test]
fn test_loro_tree_move() {
    let doc = LoroDoc::new();
    let tree = doc.get_tree("myTree");
    let root = tree.create(None).unwrap();
    let child = tree.create(Some(root)).unwrap();
    for _ in 0..16 {
        tree.create(root).unwrap();
    }
    tree.get_meta(child)
        .unwrap()
        .insert("test", "test")
        .unwrap();
    tree.mov(child, root).unwrap();
}

#[test]
fn test_export_json_updates_in_shallow_snapshot() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.get_text("text").insert(0, "123").unwrap();
    let snapshot = doc
        .export(ExportMode::shallow_snapshot_since(ID::new(1, 2)))
        .unwrap();
    let new_doc = LoroDoc::new();
    new_doc.import(&snapshot).unwrap();
    new_doc.export_json_updates(&Default::default(), &new_doc.oplog_vv());
}

#[test]
fn should_call_subscription_after_diff() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.get_text("text").insert(0, "Hello").unwrap();
    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();
    let sub = doc.subscribe_root(Arc::new(move |_| {
        called_clone.store(true, Ordering::SeqCst);
    }));
    sub.detach();
    doc.diff(&doc.state_frontiers(), &ID::new(1, 3).into())
        .unwrap();

    doc.get_text("text").insert(0, "Hello").unwrap();
    doc.commit();
    assert!(called.load(Ordering::SeqCst));
}

#[test]
fn test_get_value_by_path() {
    let doc = LoroDoc::new();
    let tree = doc.get_tree("tree");

    // Create a tree structure:
    // root
    //  |- child1
    //  |   |- grandchild
    //  |- child2
    let root = tree.create(TreeParentId::Root).unwrap();
    let child1 = tree.create(TreeParentId::Node(root)).unwrap();
    let child2 = tree.create(TreeParentId::Node(root)).unwrap();
    let grandchild = tree.create(TreeParentId::Node(child1)).unwrap();

    // Set metadata for each node
    tree.get_meta(root).unwrap().insert("name", "root").unwrap();
    tree.get_meta(child1)
        .unwrap()
        .insert("name", "child1")
        .unwrap();
    tree.get_meta(child2)
        .unwrap()
        .insert("name", "child2")
        .unwrap();
    tree.get_meta(grandchild)
        .unwrap()
        .insert("name", "grandchild")
        .unwrap();

    // Test accessing nodes by index
    let path = vec![
        Index::Key("tree".into()),
        Index::Seq(0), // root
        Index::Seq(0), // child1
        Index::Seq(0), // grandchild
    ];
    let value = doc.get_by_path(&path).unwrap();
    let map = value.into_container().unwrap().into_map().unwrap();
    assert_eq!(
        map.get("name")
            .unwrap()
            .as_value()
            .unwrap()
            .as_string()
            .unwrap()
            .as_str(),
        "grandchild"
    );

    // Test accessing nodes by ID
    let path = vec![
        Index::Key("tree".into()),
        Index::Node(root),
        Index::Node(child1),
        Index::Node(grandchild),
    ];
    let value = doc.get_by_path(&path).unwrap();
    let map = value.into_container().unwrap().into_map().unwrap();
    assert_eq!(
        map.get("name")
            .unwrap()
            .as_value()
            .unwrap()
            .as_string()
            .unwrap()
            .as_str(),
        "grandchild"
    );

    // Test accessing node metadata directly
    let path = vec![
        Index::Key("tree".into()),
        Index::Node(root),
        Index::Key("name".into()),
    ];
    let value = doc.get_by_path(&path).unwrap();
    assert_eq!(
        value.into_value().unwrap().as_string().unwrap().as_str(),
        "root"
    );

    // Test accessing node metadata through index
    let path = vec![
        Index::Key("tree".into()),
        Index::Seq(0), // root
        Index::Key("name".into()),
    ];
    let value = doc.get_by_path(&path).unwrap();
    assert_eq!(
        value.into_value().unwrap().as_string().unwrap().as_str(),
        "root"
    );
}

#[test]
fn test_by_str_path() {
    let doc = LoroDoc::new();
    let tree = doc.get_tree("tree");
    let root = tree.create(TreeParentId::Root).unwrap();
    let child = tree.create(TreeParentId::Node(root)).unwrap();
    let grandchild = tree.create(TreeParentId::Node(child)).unwrap();
    tree.get_meta(grandchild)
        .unwrap()
        .insert("type", "grandChild")
        .unwrap();
    let container = doc.get_by_str_path("tree/0/0/0").unwrap();
    assert!(container.is_container());
    let map = container.into_container().unwrap().into_map().unwrap();
    assert_eq!(
        map.get("type")
            .unwrap()
            .as_value()
            .unwrap()
            .as_string()
            .unwrap()
            .as_str(),
        "grandChild"
    );
    let value = doc.get_by_str_path("tree/0/0/0/type").unwrap();
    assert_eq!(
        value.into_value().unwrap().as_string().unwrap().as_str(),
        "grandChild"
    );
}

#[test]
fn test_memory_leak() {
    fn repeat(f: impl Fn(), n: usize) {
        for _ in 0..n {
            f();
        }
    }

    let s = "h".repeat(100_000);
    repeat(
        || {
            let doc = LoroDoc::new();
            doc.get_text("text").insert(0, &s).unwrap();
            doc.commit();
        },
        10,
    );

    assert!(
        dev_utils::get_mem_usage() < ByteSize(1_000_000),
        "memory usage should be less than 1MB {:?}",
        dev_utils::get_mem_usage()
    );
}

#[test]
fn test_iter_change_on_edge() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.set_change_merge_interval(10);
    doc.get_text("text").insert(0, "hello").unwrap();
    doc.commit_with(CommitOptions::default().timestamp(1000));
    doc.get_text("text").insert(0, "hello").unwrap();
    doc.commit_with(CommitOptions::default().timestamp(3000));
    doc.get_text("text").insert(0, "hello").unwrap();
    doc.commit_with(CommitOptions::default().timestamp(5000));
    doc.set_peer_id(2).unwrap();
    doc.get_text("text").insert(0, "hello").unwrap();
    doc.commit_with(CommitOptions::default().timestamp(6000));
    doc.fork_at(&Frontiers::from_id(ID::new(1, 9)));
    doc.fork_at(&Frontiers::from_id(ID::new(1, 10)));
    doc.fork_at(&Frontiers::from_id(ID::new(1, 11)));
}

#[test]
fn test_to_delta_on_detached_text() {
    let text = LoroText::new();
    text.insert(0, "Hello").unwrap();
    text.mark(0..5, "bold", true).unwrap();
    let delta = text.to_delta();
    assert_eq!(
        delta,
        vec![TextDelta::Insert {
            insert: "Hello".to_string(),
            attributes: Some(fx_map! { "bold".into() => LoroValue::Bool(true) }),
        }]
    );
}

#[test]
fn test_apply_delta_on_the_end() {
    let doc = LoroDoc::new();
    doc.get_text("text").insert(0, "Hello").unwrap();
    doc.get_text("text").apply_delta(&[
        TextDelta::Retain {
            retain: 5,
            attributes: None,
        },
        TextDelta::Retain {
            retain: 1,
            attributes: Some(fx_map! { "bold".into() => LoroValue::Bool(true),  "italic".into() => LoroValue::Bool(true)  }),
        },
    ]).unwrap();
    assert_eq!(
        doc.get_text("text").to_delta(),
        vec![
            TextDelta::Insert {
                insert: "Hello".to_string(),
                attributes: None,
            },
            TextDelta::Insert {
                insert: "\n".to_string(),
                attributes: Some(
                    fx_map! { "bold".into() => LoroValue::Bool(true), "italic".into() => LoroValue::Bool(true) }
                ),
            },
        ]
    );
}

#[test]
fn test_delete_root_containers() {
    let doc = LoroDoc::new();
    let _map = doc.get_map("map");
    doc.get_map("m");
    let _text = doc.get_text("text");
    doc.delete_root_container(ContainerID::new_root("map", ContainerType::Map));
    doc.delete_root_container(ContainerID::new_root("text", ContainerType::Text));
    let mut m = LoroMapValue::default();
    m.make_mut()
        .insert("m".into(), LoroValue::Map(LoroMapValue::default()));
    assert_eq!(doc.get_deep_value(), LoroValue::Map(m.clone()));
    let snapshot = doc.export(ExportMode::Snapshot).unwrap();
    let new_doc = LoroDoc::new();
    new_doc.import(&snapshot).unwrap();
    assert_eq!(new_doc.get_deep_value(), LoroValue::Map(m.clone()));
}

#[test]
fn test_hide_empty_root_containers() {
    let doc = LoroDoc::new();
    let _map = doc.get_map("map");
    let mut expected = LoroMapValue::default();
    expected
        .make_mut()
        .insert("map".into(), LoroValue::Map(LoroMapValue::default()));
    assert_eq!(doc.get_deep_value(), LoroValue::Map(expected));

    doc.set_hide_empty_root_containers(true);
    assert_eq!(
        doc.get_deep_value(),
        LoroValue::Map(LoroMapValue::default())
    );
}

#[test]
fn test_from_shallow_snapshot() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.get_text("text").insert(0, "Hello").unwrap();
    doc.commit();
    let snapshot = doc
        .export(ExportMode::shallow_snapshot_owned(doc.state_frontiers()))
        .unwrap();
    let new_doc = LoroDoc::from_snapshot(&snapshot).unwrap();
    let mut expected = LoroMapValue::default();
    expected
        .make_mut()
        .insert("text".into(), LoroValue::String("Hello".into()));
    assert_eq!(new_doc.get_deep_value(), LoroValue::Map(expected));
}

#[test]
fn test_checkout_to_unknown_version() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    doc.get_text("text").insert(0, "Hello").unwrap();
    let result = doc.checkout(&Frontiers::from([ID::new(2, 2), ID::new(1, 1)]));
    assert!(result.is_err());
    assert!(matches!(
        result.err().unwrap(),
        LoroError::FrontiersNotFound(..)
    ));
}
