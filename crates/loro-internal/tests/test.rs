use loro_common::ID;
use loro_internal::{version::Frontiers, LoroDoc};

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
fn test_checkout() {
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
}
