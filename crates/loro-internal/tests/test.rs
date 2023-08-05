use loro_common::ID;
use loro_internal::{version::Frontiers, LoroDoc, ToJson};

#[test]
fn test_timestamp() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    let mut txn = doc.txn().unwrap();
    text.insert(&mut txn, 0, "123").unwrap();
    txn.commit().unwrap();
    let change = doc
        .oplog()
        .lock()
        .unwrap()
        .get_change_at(ID::new(doc.peer_id(), 0))
        .unwrap();
    assert!(change.timestamp() > 1690966970);
}

#[test]
fn test_text_checkout() {
    let mut doc = LoroDoc::new();
    let text = doc.get_text("text");
    let mut txn = doc.txn().unwrap();
    text.insert(&mut txn, 0, "你界").unwrap();
    text.insert(&mut txn, 1, "好世").unwrap();
    txn.commit().unwrap();
    {
        doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 0)].as_slice()));
        assert_eq!(text.get_value().as_string().unwrap().as_str(), "你");
    }
    {
        doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 1)].as_slice()));
        assert_eq!(text.get_value().as_string().unwrap().as_str(), "你界");
    }
    {
        doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 2)].as_slice()));
        assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好界");
    }
    {
        doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 3)].as_slice()));
        assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好世界");
    }
    assert_eq!(text.len_unicode(), 4);
    assert_eq!(text.len_utf8(), 12);
    assert_eq!(text.len_unicode(), 4);

    doc.checkout_to_latest();
    doc.with_txn(|txn| text.delete(txn, 3, 1)).unwrap();
    assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好世");
    doc.with_txn(|txn| text.delete(txn, 2, 1)).unwrap();
    assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好");
    doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 3)].as_slice()));
    assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好世界");
    doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 4)].as_slice()));
    assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好世");
    doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 5)].as_slice()));
    assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好");
    {
        doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 0)].as_slice()));
        assert_eq!(text.get_value().as_string().unwrap().as_str(), "你");
    }
    {
        doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 1)].as_slice()));
        assert_eq!(text.get_value().as_string().unwrap().as_str(), "你界");
    }
    {
        doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 2)].as_slice()));
        assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好界");
    }
    {
        doc.checkout(&Frontiers::from([ID::new(doc.peer_id(), 3)].as_slice()));
        assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好世界");
    }
}

#[test]
fn map_checkout() {
    let mut doc = LoroDoc::new();
    let meta = doc.get_map("meta");
    let v_empty = doc.oplog_frontiers();
    doc.with_txn(|txn| {
        meta.insert(txn, "key", 0.into()).unwrap();
        Ok(())
    })
    .unwrap();
    let v0 = doc.oplog_frontiers();
    doc.with_txn(|txn| {
        meta.insert(txn, "key", 1.into()).unwrap();
        Ok(())
    })
    .unwrap();
    let v1 = doc.oplog_frontiers();
    assert_eq!(meta.get_deep_value().to_json(), r#"{"key":1}"#);
    doc.checkout(&v0);
    assert_eq!(meta.get_deep_value().to_json(), r#"{"key":0}"#);
    doc.checkout(&v_empty);
    assert_eq!(meta.get_deep_value().to_json(), r#"{}"#);
    doc.checkout(&v1);
    assert_eq!(meta.get_deep_value().to_json(), r#"{"key":1}"#);
}

#[test]
fn map_concurrent_checkout() {
    let mut doc_a = LoroDoc::new();
    let meta_a = doc_a.get_map("meta");
    let doc_b = LoroDoc::new();
    let meta_b = doc_b.get_map("meta");

    doc_a
        .with_txn(|txn| {
            meta_a.insert(txn, "key", 0.into()).unwrap();
            Ok(())
        })
        .unwrap();
    let va = doc_a.oplog_frontiers();
    doc_b
        .with_txn(|txn| {
            meta_b.insert(txn, "s", 1.into()).unwrap();
            Ok(())
        })
        .unwrap();
    let vb_0 = doc_b.oplog_frontiers();
    doc_b
        .with_txn(|txn| {
            meta_b.insert(txn, "key", 1.into()).unwrap();
            Ok(())
        })
        .unwrap();
    let vb_1 = doc_b.oplog_frontiers();
    doc_a.import(&doc_b.export_snapshot()).unwrap();
    doc_a
        .with_txn(|txn| {
            meta_a.insert(txn, "key", 2.into()).unwrap();
            Ok(())
        })
        .unwrap();

    let v_merged = doc_a.oplog_frontiers();

    doc_a.checkout(&va);
    assert_eq!(meta_a.get_deep_value().to_json(), r#"{"key":0}"#);
    doc_a.checkout(&vb_0);
    assert_eq!(meta_a.get_deep_value().to_json(), r#"{"s":1}"#);
    doc_a.checkout(&vb_1);
    assert_eq!(meta_a.get_deep_value().to_json(), r#"{"s":1,"key":1}"#);
    doc_a.checkout(&v_merged);
    assert_eq!(meta_a.get_deep_value().to_json(), r#"{"s":1,"key":2}"#);
}
