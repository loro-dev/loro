use std::sync::{atomic::AtomicBool, Arc, Mutex};

use loro_common::{ContainerID, ContainerType, LoroValue, ID};
use loro_internal::{
    container::richtext::TextStyleInfoFlag, version::Frontiers, ApplyDiff, LoroDoc, ToJson,
};
use serde_json::json;

#[test]
fn event_from_checkout() {
    let a = LoroDoc::new_auto_commit();
    let sub_id = a.subscribe_root(Arc::new(|event| {
        assert!(!event.doc.from_checkout);
    }));
    a.get_text("text").insert(0, "hello").unwrap();
    a.commit_then_renew();
    let version = a.oplog_frontiers();
    a.get_text("text").insert(0, "hello").unwrap();
    a.commit_then_renew();
    a.unsubscribe(sub_id);
    let ran = Arc::new(AtomicBool::new(false));
    let ran_cloned = ran.clone();
    a.subscribe_root(Arc::new(move |event| {
        assert!(event.doc.from_checkout);
        ran.store(true, std::sync::atomic::Ordering::Relaxed);
    }));
    a.checkout(&version).unwrap();
    assert!(ran_cloned.load(std::sync::atomic::Ordering::Relaxed));
}

#[test]
fn out_of_bound_test() {
    let a = LoroDoc::new_auto_commit();
    a.get_text("text").insert(0, "Hello").unwrap();
    a.get_list("list").insert(0, "Hello").unwrap();
    a.get_list("list").insert(1, "Hello").unwrap();
    // expect out of bound err
    let err = a.get_text("text").insert(6, "Hello").unwrap_err();
    assert!(matches!(err, loro_common::LoroError::OutOfBound { .. }));
    let err = a.get_text("text").delete(3, 5).unwrap_err();
    assert!(matches!(err, loro_common::LoroError::OutOfBound { .. }));
    let err = a
        .get_text("text")
        .mark(0, 8, "h", 5.into(), TextStyleInfoFlag::BOLD)
        .unwrap_err();
    assert!(matches!(err, loro_common::LoroError::OutOfBound { .. }));
    let _err = a
        .get_text("text")
        .mark(3, 0, "h", 5.into(), TextStyleInfoFlag::BOLD)
        .unwrap_err();
    let err = a.get_list("list").insert(6, "Hello").unwrap_err();
    assert!(matches!(err, loro_common::LoroError::OutOfBound { .. }));
    let err = a.get_list("list").delete(3, 2).unwrap_err();
    assert!(matches!(err, loro_common::LoroError::OutOfBound { .. }));
    let err = a
        .get_list("list")
        .insert_container(3, ContainerType::Map)
        .unwrap_err();
    assert!(matches!(err, loro_common::LoroError::OutOfBound { .. }));
}

#[test]
fn list() {
    let a = LoroDoc::new_auto_commit();
    a.get_list("list").insert(0, "Hello").unwrap();
    assert_eq!(a.get_list("list").get(0).unwrap(), LoroValue::from("Hello"));
    let map = a
        .get_list("list")
        .insert_container(1, ContainerType::Map)
        .unwrap()
        .into_map()
        .unwrap();
    map.insert("Hello", LoroValue::from("u")).unwrap();
    let pos = map
        .insert_container("pos", ContainerType::Map)
        .unwrap()
        .into_map()
        .unwrap();
    pos.insert("x", 0).unwrap();
    pos.insert("y", 100).unwrap();

    let cid = map.id();
    let id = a.get_list("list").get(1);
    assert_eq!(id.as_ref().unwrap().as_container().unwrap(), &cid);
    let map = a.get_map(id.unwrap().into_container().unwrap());
    let new_pos = a.get_map(map.get("pos").unwrap().into_container().unwrap());
    assert_eq!(
        new_pos.get_deep_value().to_json_value(),
        json!({
            "x": 0,
            "y": 100,
        })
    );
}

#[test]
fn richtext_mark_event() {
    let a = LoroDoc::new_auto_commit();
    a.subscribe(
        &a.get_text("text").id(),
        Arc::new(|e| {
            let delta = e.container.diff.as_text().unwrap();
            assert_eq!(
                delta.to_json_value(),
                json!([
                        {"insert": "He", "attributes": {"bold": true}},
                        {"insert": "ll", "attributes": {"bold": null}},
                        {"insert": "o", "attributes": {"bold": true}}
                ])
            )
        }),
    );
    a.get_text("text").insert(0, "Hello").unwrap();
    a.get_text("text")
        .mark(0, 5, "bold", true.into(), TextStyleInfoFlag::BOLD)
        .unwrap();
    a.get_text("text")
        .mark(
            2,
            4,
            "bold",
            LoroValue::Null,
            TextStyleInfoFlag::BOLD.to_delete(),
        )
        .unwrap();
    a.commit_then_stop();
    let b = LoroDoc::new_auto_commit();
    b.subscribe(
        &a.get_text("text").id(),
        Arc::new(|e| {
            let delta = e.container.diff.as_text().unwrap();
            assert_eq!(
                delta.to_json_value(),
                json!([
                    {"insert": "He", "attributes": {"bold": true}},
                    {"insert": "ll", "attributes": {"bold": null}},
                    {"insert": "o", "attributes": {"bold": true}}
                ])
            )
        }),
    );
    b.merge(&a).unwrap();
}

#[test]
fn concurrent_richtext_mark_event() {
    let a = LoroDoc::new_auto_commit();
    let b = LoroDoc::new_auto_commit();
    let c = LoroDoc::new_auto_commit();
    a.get_text("text").insert(0, "Hello").unwrap();
    b.merge(&a).unwrap();
    c.merge(&a).unwrap();
    b.get_text("text")
        .mark(0, 3, "bold", true.into(), TextStyleInfoFlag::BOLD)
        .unwrap();
    c.get_text("text")
        .mark(1, 4, "link", true.into(), TextStyleInfoFlag::LINK)
        .unwrap();
    b.merge(&c).unwrap();
    let sub_id = a.subscribe(
        &a.get_text("text").id(),
        Arc::new(|e| {
            let delta = e.container.diff.as_text().unwrap();
            assert_eq!(
                delta.to_json_value(),
                json!([
                    {"retain": 1, "attributes": {"bold": true, }},
                    {"retain": 2, "attributes": {"bold": true, "link": true}},
                    {"retain": 1, "attributes": {"link": true}},
                ])
            )
        }),
    );

    a.merge(&b).unwrap();
    a.unsubscribe(sub_id);

    let sub_id = a.subscribe(
        &a.get_text("text").id(),
        Arc::new(|e| {
            let delta = e.container.diff.as_text().unwrap();
            assert_eq!(
                delta.to_json_value(),
                json!([
                    {
                        "retain": 2,
                    },
                    {
                        "retain": 1,
                        "attributes": {"bold": null}
                    }
                ])
            )
        }),
    );

    b.get_text("text")
        .mark(
            2,
            3,
            "bold",
            LoroValue::Null,
            TextStyleInfoFlag::BOLD.to_delete(),
        )
        .unwrap();
    a.merge(&b).unwrap();
    a.unsubscribe(sub_id);
    a.subscribe(
        &a.get_text("text").id(),
        Arc::new(|e| {
            let delta = e.container.diff.as_text().unwrap();
            assert_eq!(
                delta.to_json_value(),
                json!([
                    {
                        "retain": 2,
                    },
                    {
                        "insert": "A",
                        "attributes": {"bold": true, "link": true}
                    }
                ])
            )
        }),
    );
    a.get_text("text").insert(2, "A").unwrap();
    a.commit_then_stop();
}

#[test]
fn insert_richtext_event() {
    let a = LoroDoc::new_auto_commit();
    a.get_text("text").insert(0, "Hello").unwrap();
    a.get_text("text")
        .mark(0, 5, "bold", true.into(), TextStyleInfoFlag::BOLD)
        .unwrap();
    a.commit_then_renew();
    let text = a.get_text("text");
    a.subscribe(
        &text.id(),
        Arc::new(|e| {
            let delta = e.container.diff.as_text().unwrap();
            assert_eq!(
                delta.to_json_value(),
                json!([
                        {"retain": 5,},
                        {"insert": " World!", "attributes": {"bold": true}}
                ])
            )
        }),
    );

    text.insert(5, " World!").unwrap();
}

#[test]
fn import_after_init_handlers() {
    let a = LoroDoc::new_auto_commit();
    a.subscribe(
        &ContainerID::new_root("text", ContainerType::Text),
        Arc::new(|event| {
            assert!(matches!(
                event.container.diff,
                loro_internal::event::Diff::Text(_)
            ))
        }),
    );
    a.subscribe(
        &ContainerID::new_root("map", ContainerType::Map),
        Arc::new(|event| {
            assert!(matches!(
                event.container.diff,
                loro_internal::event::Diff::NewMap(_)
            ))
        }),
    );
    a.subscribe(
        &ContainerID::new_root("list", ContainerType::List),
        Arc::new(|event| {
            assert!(matches!(
                event.container.diff,
                loro_internal::event::Diff::List(_)
            ))
        }),
    );

    let b = LoroDoc::new_auto_commit();
    b.get_list("list").insert(0, "list").unwrap();
    b.get_list("list_a").insert(0, "list_a").unwrap();
    b.get_text("text").insert(0, "text").unwrap();
    b.get_map("map").insert("m", "map").unwrap();
    a.import(&b.export_snapshot()).unwrap();
    a.commit_then_renew();
}

#[test]
fn test_from_snapshot() {
    let a = LoroDoc::new_auto_commit();
    a.get_text("text").insert(0, "0").unwrap();
    let snapshot = a.export_snapshot();
    let c = LoroDoc::from_snapshot(&snapshot).unwrap();
    assert_eq!(a.get_deep_value(), c.get_deep_value());
    assert_eq!(a.oplog_frontiers(), c.oplog_frontiers());
    assert_eq!(a.state_frontiers(), c.state_frontiers());
    let updates = a.export_from(&Default::default());
    let d = match LoroDoc::from_snapshot(&updates) {
        Ok(_) => panic!(),
        Err(e) => e,
    };
    assert!(matches!(d, loro_common::LoroError::DecodeError(..)));
}

#[test]
fn test_pending() {
    let a = LoroDoc::new_auto_commit();
    a.set_peer_id(0).unwrap();
    a.get_text("text").insert(0, "0").unwrap();
    let b = LoroDoc::new_auto_commit();
    b.set_peer_id(1).unwrap();
    b.import(&a.export_from(&Default::default())).unwrap();
    b.get_text("text").insert(0, "1").unwrap();
    let c = LoroDoc::new_auto_commit();
    b.set_peer_id(2).unwrap();
    c.import(&b.export_from(&Default::default())).unwrap();
    c.get_text("text").insert(0, "2").unwrap();

    // c creates a pending change for a, insert "2" cannot be merged into a yet
    a.import(&c.export_from(&b.oplog_vv())).unwrap();
    assert_eq!(a.get_deep_value().to_json_value(), json!({"text": "0"}));

    // b does not has c's change
    a.import(&b.export_from(&a.oplog_vv())).unwrap();
    dbg!(&a.oplog().lock().unwrap());
    assert_eq!(a.get_deep_value().to_json_value(), json!({"text": "210"}));
}

#[test]
fn test_checkout() {
    let doc_0 = LoroDoc::new();
    doc_0.set_peer_id(0).unwrap();
    let doc_1 = LoroDoc::new();
    doc_1.set_peer_id(1).unwrap();

    let value: Arc<Mutex<LoroValue>> = Arc::new(Mutex::new(LoroValue::Map(Default::default())));
    let root_value = value.clone();
    doc_0.subscribe_root(Arc::new(move |event| {
        let mut root_value = root_value.lock().unwrap();
        root_value.apply(
            &event.container.path.iter().map(|x| x.1.clone()).collect(),
            &[event.container.diff.clone()],
        );
    }));

    let map = doc_0.get_map("map");
    doc_0
        .with_txn(|txn| {
            let handler = map.insert_container_with_txn(txn, "text", ContainerType::Text)?;
            let text = handler.into_text().unwrap();
            text.insert_with_txn(txn, 0, "123")
        })
        .unwrap();

    let map = doc_1.get_map("map");
    doc_1
        .with_txn(|txn| map.insert_with_txn(txn, "text", LoroValue::Double(1.0)))
        .unwrap();

    doc_0
        .import(&doc_1.export_from(&Default::default()))
        .unwrap();

    doc_0
        .checkout(&Frontiers::from(vec![ID::new(0, 2)]))
        .unwrap();

    assert_eq!(&doc_0.get_deep_value(), &*value.lock().unwrap());
    assert_eq!(
        value.lock().unwrap().to_json_value(),
        json!({
            "map": {
                "text": "12"
            }
        })
    );
}

#[test]
fn import() {
    let doc = LoroDoc::new();
    doc.import(&[
        108, 111, 114, 111, 0, 0, 10, 10, 255, 255, 68, 255, 255, 4, 0, 5, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 255, 255, 108, 111, 114, 111, 255, 255, 0, 255, 207, 207, 255, 255, 255, 255,
        255,
    ])
    .unwrap_or_default();
}

#[test]
fn test_timestamp() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    let mut txn = doc.txn().unwrap();
    text.insert_with_txn(&mut txn, 0, "123").unwrap();
    txn.commit().unwrap();
    let op_log = &doc.oplog().lock().unwrap();
    let change = op_log.get_change_at(ID::new(doc.peer_id(), 0)).unwrap();
    assert!(change.timestamp() > 1690966970);
}

#[test]
fn test_text_checkout() {
    let doc = LoroDoc::new();
    doc.set_peer_id(1).unwrap();
    let text = doc.get_text("text");
    let mut txn = doc.txn().unwrap();
    text.insert_with_txn(&mut txn, 0, "你界").unwrap();
    text.insert_with_txn(&mut txn, 1, "好世").unwrap();
    txn.commit().unwrap();
    {
        doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 0)].as_slice()))
            .unwrap();
        assert_eq!(text.get_value().as_string().unwrap().as_str(), "你");
    }
    {
        doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 1)].as_slice()))
            .unwrap();
        assert_eq!(text.get_value().as_string().unwrap().as_str(), "你界");
    }
    {
        doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 2)].as_slice()))
            .unwrap();
        assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好界");
    }
    {
        doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 3)].as_slice()))
            .unwrap();
        assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好世界");
    }
    assert_eq!(text.len_unicode(), 4);
    assert_eq!(text.len_utf8(), 12);
    assert_eq!(text.len_unicode(), 4);

    doc.checkout_to_latest();
    doc.with_txn(|txn| text.delete_with_txn(txn, 3, 1)).unwrap();
    assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好世");
    doc.with_txn(|txn| text.delete_with_txn(txn, 2, 1)).unwrap();
    assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好");
    doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 3)].as_slice()))
        .unwrap();
    assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好世界");
    doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 4)].as_slice()))
        .unwrap();
    assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好世");
    doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 5)].as_slice()))
        .unwrap();
    assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好");
    {
        doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 0)].as_slice()))
            .unwrap();
        assert_eq!(text.get_value().as_string().unwrap().as_str(), "你");
    }
    {
        doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 1)].as_slice()))
            .unwrap();
        assert_eq!(text.get_value().as_string().unwrap().as_str(), "你界");
    }
    {
        doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 2)].as_slice()))
            .unwrap();
        assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好界");
    }
    {
        doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 3)].as_slice()))
            .unwrap();
        assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好世界");
    }
}

#[test]
fn map_checkout() {
    let doc = LoroDoc::new();
    let meta = doc.get_map("meta");
    let v_empty = doc.oplog_frontiers();
    doc.with_txn(|txn| {
        meta.insert_with_txn(txn, "key", 0.into()).unwrap();
        Ok(())
    })
    .unwrap();
    let v0 = doc.oplog_frontiers();
    doc.with_txn(|txn| {
        meta.insert_with_txn(txn, "key", 1.into()).unwrap();
        Ok(())
    })
    .unwrap();
    let v1 = doc.oplog_frontiers();
    assert_eq!(meta.get_deep_value().to_json(), r#"{"key":1}"#);
    doc.checkout(&v0).unwrap();
    assert_eq!(meta.get_deep_value().to_json(), r#"{"key":0}"#);
    doc.checkout(&v_empty).unwrap();
    assert_eq!(meta.get_deep_value().to_json(), r#"{}"#);
    doc.checkout(&v1).unwrap();
    assert_eq!(meta.get_deep_value().to_json(), r#"{"key":1}"#);
}

#[test]
fn a_list_of_map_checkout() {
    let doc = LoroDoc::new();
    let entry = doc.get_map("entry");
    let (list, sub) = doc
        .with_txn(|txn| {
            let list = entry
                .insert_container_with_txn(txn, "list", loro_common::ContainerType::List)?
                .into_list()
                .unwrap();
            let sub_map = list
                .insert_container_with_txn(txn, 0, loro_common::ContainerType::Map)?
                .into_map()
                .unwrap();
            sub_map.insert_with_txn(txn, "x", 100.into())?;
            sub_map.insert_with_txn(txn, "y", 1000.into())?;
            Ok((list, sub_map))
        })
        .unwrap();
    let v0 = doc.oplog_frontiers();
    let d0 = doc.get_deep_value().to_json();
    doc.with_txn(|txn| {
        list.insert_with_txn(txn, 0, 3.into())?;
        list.push_with_txn(txn, 4.into())?;
        list.insert_container_with_txn(txn, 2, loro_common::ContainerType::Map)?;
        list.insert_container_with_txn(txn, 3, loro_common::ContainerType::Map)?;
        Ok(())
    })
    .unwrap();
    doc.with_txn(|txn| {
        list.delete_with_txn(txn, 2, 1)?;
        Ok(())
    })
    .unwrap();
    doc.with_txn(|txn| {
        sub.insert_with_txn(txn, "x", 9.into())?;
        sub.insert_with_txn(txn, "y", 9.into())?;
        Ok(())
    })
    .unwrap();
    doc.with_txn(|txn| {
        sub.insert_with_txn(txn, "z", 9.into())?;
        Ok(())
    })
    .unwrap();
    let v1 = doc.oplog_frontiers();
    let d1 = doc.get_deep_value().to_json();
    doc.with_txn(|txn| {
        sub.insert_with_txn(txn, "x", 77.into())?;
        Ok(())
    })
    .unwrap();
    doc.with_txn(|txn| {
        sub.insert_with_txn(txn, "y", 88.into())?;
        Ok(())
    })
    .unwrap();
    doc.with_txn(|txn| {
        list.delete_with_txn(txn, 0, 1)?;
        list.insert_with_txn(txn, 0, 123.into())?;
        list.push_with_txn(txn, 99.into())?;
        Ok(())
    })
    .unwrap();
    let v2 = doc.oplog_frontiers();
    let d2 = doc.get_deep_value().to_json();

    doc.checkout(&v0).unwrap();
    assert_eq!(doc.get_deep_value().to_json(), d0);
    doc.checkout(&v1).unwrap();
    assert_eq!(doc.get_deep_value().to_json(), d1);
    doc.checkout(&v2).unwrap();
    println!("{}", doc.get_deep_value_with_id().to_json_pretty());
    assert_eq!(doc.get_deep_value().to_json(), d2);
    debug_log::group!("checking out v1");
    doc.checkout(&v1).unwrap();
    debug_log::group_end!();
    println!("{}", doc.get_deep_value_with_id().to_json_pretty());
    assert_eq!(doc.get_deep_value().to_json(), d1);
    doc.checkout(&v0).unwrap();
    assert_eq!(doc.get_deep_value().to_json(), d0);
}

#[test]
fn map_concurrent_checkout() {
    let doc_a = LoroDoc::new();
    let meta_a = doc_a.get_map("meta");
    let doc_b = LoroDoc::new();
    let meta_b = doc_b.get_map("meta");

    doc_a
        .with_txn(|txn| {
            meta_a.insert_with_txn(txn, "key", 0.into()).unwrap();
            Ok(())
        })
        .unwrap();
    let va = doc_a.oplog_frontiers();
    doc_b
        .with_txn(|txn| {
            meta_b.insert_with_txn(txn, "s", 1.into()).unwrap();
            Ok(())
        })
        .unwrap();
    let vb_0 = doc_b.oplog_frontiers();
    doc_b
        .with_txn(|txn| {
            meta_b.insert_with_txn(txn, "key", 1.into()).unwrap();
            Ok(())
        })
        .unwrap();
    let vb_1 = doc_b.oplog_frontiers();
    doc_a.import(&doc_b.export_snapshot()).unwrap();
    doc_a
        .with_txn(|txn| {
            meta_a.insert_with_txn(txn, "key", 2.into()).unwrap();
            Ok(())
        })
        .unwrap();

    let v_merged = doc_a.oplog_frontiers();

    doc_a.checkout(&va).unwrap();
    assert_eq!(meta_a.get_deep_value().to_json(), r#"{"key":0}"#);
    doc_a.checkout(&vb_0).unwrap();
    assert_eq!(meta_a.get_deep_value().to_json(), r#"{"s":1}"#);
    doc_a.checkout(&vb_1).unwrap();
    assert_eq!(meta_a.get_deep_value().to_json(), r#"{"s":1,"key":1}"#);
    doc_a.checkout(&v_merged).unwrap();
    assert_eq!(meta_a.get_deep_value().to_json(), r#"{"s":1,"key":2}"#);
}

#[test]
fn tree_checkout() {
    let doc_a = LoroDoc::new();
    doc_a.subscribe_root(Arc::new(|_e| {}));
    doc_a.set_peer_id(1).unwrap();
    let tree = doc_a.get_tree("root");
    let id1 = doc_a
        .with_txn(|txn| tree.create_with_txn(txn, None))
        .unwrap();
    let id2 = doc_a
        .with_txn(|txn| tree.create_with_txn(txn, id1))
        .unwrap();
    let v1_state = tree.get_deep_value();
    let v1 = doc_a.oplog_frontiers();
    let _id3 = doc_a
        .with_txn(|txn| tree.create_with_txn(txn, id2))
        .unwrap();
    let v2_state = tree.get_deep_value();
    let v2 = doc_a.oplog_frontiers();
    doc_a
        .with_txn(|txn| tree.delete_with_txn(txn, id2))
        .unwrap();
    let v3_state = tree.get_deep_value();
    let v3 = doc_a.oplog_frontiers();
    doc_a.checkout(&v1).unwrap();
    assert_eq!(
        serde_json::to_value(tree.get_deep_value())
            .unwrap()
            .get("roots"),
        serde_json::to_value(v1_state).unwrap().get("roots")
    );
    doc_a.checkout(&v2).unwrap();
    assert_eq!(
        serde_json::to_value(tree.get_deep_value())
            .unwrap()
            .get("roots"),
        serde_json::to_value(v2_state).unwrap().get("roots")
    );
    doc_a.checkout(&v3).unwrap();
    assert_eq!(
        serde_json::to_value(tree.get_deep_value())
            .unwrap()
            .get("roots"),
        serde_json::to_value(v3_state).unwrap().get("roots")
    );

    doc_a.attach();
    doc_a
        .with_txn(|txn| {
            tree.create_with_txn(txn, None)
            //tree.insert_meta(txn, id1, "a", 1.into())
        })
        .unwrap();
}
