use std::fs::File;

use loro_common::ID;
use loro_internal::{version::Frontiers, LoroDoc, ToJson};

#[ctor::ctor]
fn init_color_backtrace() {
    color_backtrace::install();
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
fn import_history() {
    let doc = LoroDoc::new();
    doc.import(include_bytes!("./history_compressed_rle_updates.dat"))
        .unwrap();
    let doc2 = LoroDoc::new();
    doc2.import(include_bytes!("./history_snapshot.dat"))
        .unwrap();
}

#[test]
fn test_timestamp() {
    let doc = LoroDoc::new();
    let text = doc.get_text("text");
    let mut txn = doc.txn().unwrap();
    text.insert(&mut txn, 0, "123").unwrap();
    txn.commit().unwrap();
    let op_log = &doc.oplog().lock().unwrap();
    let change = op_log.get_change_at(ID::new(doc.peer_id(), 0)).unwrap();
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
    doc.with_txn(|txn| text.delete(txn, 3, 1)).unwrap();
    assert_eq!(text.get_value().as_string().unwrap().as_str(), "你好世");
    doc.with_txn(|txn| text.delete(txn, 2, 1)).unwrap();
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
    doc.checkout(&v0).unwrap();
    assert_eq!(meta.get_deep_value().to_json(), r#"{"key":0}"#);
    doc.checkout(&v_empty).unwrap();
    assert_eq!(meta.get_deep_value().to_json(), r#"{}"#);
    doc.checkout(&v1).unwrap();
    assert_eq!(meta.get_deep_value().to_json(), r#"{"key":1}"#);
}

#[test]
fn a_list_of_map_checkout() {
    let mut doc = LoroDoc::new();
    let entry = doc.get_map("entry");
    let (list, sub) = doc
        .with_txn(|txn| {
            let list = entry
                .insert_container(txn, "list", loro_common::ContainerType::List)?
                .into_list()
                .unwrap();
            let sub_map = list
                .insert_container(txn, 0, loro_common::ContainerType::Map)?
                .into_map()
                .unwrap();
            sub_map.insert(txn, "x", 100.into())?;
            sub_map.insert(txn, "y", 1000.into())?;
            Ok((list, sub_map))
        })
        .unwrap();
    let v0 = doc.oplog_frontiers();
    let d0 = doc.get_deep_value().to_json();
    doc.with_txn(|txn| {
        list.insert(txn, 0, 3.into())?;
        list.push(txn, 4.into())?;
        list.insert_container(txn, 2, loro_common::ContainerType::Map)?;
        list.insert_container(txn, 3, loro_common::ContainerType::Map)?;
        Ok(())
    })
    .unwrap();
    doc.with_txn(|txn| {
        list.delete(txn, 2, 1)?;
        Ok(())
    })
    .unwrap();
    doc.with_txn(|txn| {
        sub.insert(txn, "x", 9.into())?;
        sub.insert(txn, "y", 9.into())?;
        Ok(())
    })
    .unwrap();
    doc.with_txn(|txn| {
        sub.insert(txn, "z", 9.into())?;
        Ok(())
    })
    .unwrap();
    let v1 = doc.oplog_frontiers();
    let d1 = doc.get_deep_value().to_json();
    doc.with_txn(|txn| {
        sub.insert(txn, "x", 77.into())?;
        Ok(())
    })
    .unwrap();
    doc.with_txn(|txn| {
        sub.insert(txn, "y", 88.into())?;
        Ok(())
    })
    .unwrap();
    doc.with_txn(|txn| {
        list.delete(txn, 0, 1)?;
        list.insert(txn, 0, 123.into())?;
        list.push(txn, 99.into())?;
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

    doc_a.checkout(&va).unwrap();
    assert_eq!(meta_a.get_deep_value().to_json(), r#"{"key":0}"#);
    doc_a.checkout(&vb_0).unwrap();
    assert_eq!(meta_a.get_deep_value().to_json(), r#"{"s":1}"#);
    doc_a.checkout(&vb_1).unwrap();
    assert_eq!(meta_a.get_deep_value().to_json(), r#"{"s":1,"key":1}"#);
    doc_a.checkout(&v_merged).unwrap();
    assert_eq!(meta_a.get_deep_value().to_json(), r#"{"s":1,"key":2}"#);
}
